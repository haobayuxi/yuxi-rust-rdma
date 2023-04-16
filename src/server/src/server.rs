//! An rpc server that process the message sent by client.
//!
//! An rpc server provides some service and wait for the client's requests.
//! The client connect rpc server first and then send requets to the server.
//!
//! You can try this example by running:
//!
//!     cargo run --example rpc

use async_rdma::{LocalMr, LocalMrReadAccess, LocalMrWriteAccess, Rdma, RdmaBuilder};
use std::{alloc::Layout, sync::Arc, time::Duration};
use tokio::net::ToSocketAddrs;

#[derive(Clone, Debug)]
enum Request {
    Add { arg1: u32, arg2: u32 },
    Sync,
}

#[derive(Clone, Debug)]
enum Response {
    Add { res: u32 },
    Sync,
}

/// Rpc server
struct Server {}
impl Server {
    // #[tokio::main]
    async fn start<A: ToSocketAddrs>(addr: A) {
        // wait for client to connect
        let rdma = Arc::new(RdmaBuilder::default().listen(addr).await.unwrap());
        // run rpc task loop
        let sr_handler = tokio::spawn(Self::sr_task(rdma.clone()));
        // let wr_handler = tokio::spawn(Self::wr_task(rdma));
        sr_handler.await.unwrap();
        // wr_handler.await.unwrap();
        Self::wr_task(rdma).await;
    }

    /// Server can't aware of rdma `read` or `write`, so we need sync with client
    /// before client `read` and after `write`.
    async fn sync_with_client(rdma: &Rdma) {
        let mut lmr_sync = rdma
            .alloc_local_mr(Layout::new::<Request>())
            .map_err(|err| println!("{}", &err))
            .unwrap();
        //write data to lmr
        unsafe { *(*lmr_sync.as_mut_ptr() as *mut Request) = Request::Sync };
        rdma.send(&lmr_sync)
            .await
            .map_err(|err| println!("{}", &err))
            .unwrap();
    }

    /// Loop used to process rpc requests powered by rdma 'read' and 'write'
    async fn wr_task(rdma: Arc<Rdma>) {
        loop {
            // receive a lmr which was requested by client to `write` `Requests`
            let lmr_req = rdma
                .receive_local_mr()
                .await
                .map_err(|err| println!("{}", &err))
                .unwrap();
            Self::sync_with_client(&rdma).await;
            let resp = unsafe {
                // get 'Request' from lmr
                let req = &*(*lmr_req.as_ptr() as *const Request);
                Self::process_request(req)
            };
            // alloc a lmr for client to 'read' 'Response'
            let mut lmr_resp = rdma
                .alloc_local_mr(Layout::new::<Response>())
                .map_err(|err| println!("{}", &err))
                .unwrap();
            // put 'Response' into lmr
            unsafe { *(*lmr_resp.as_mut_ptr() as *mut Response) = resp };
            Self::sync_with_client(&rdma).await;
            // send the metadata of lmr to client to 'read'
            rdma.send_local_mr(lmr_resp)
                .await
                .map_err(|err| println!("{}", &err))
                .unwrap();
        }
    }
    /// Loop used to process rpc requests powered by rdma 'send' and 'receive'
    async fn sr_task(rdma: Arc<Rdma>) {
        loop {
            // wait for the `send` request sent by client
            let resp = rdma
                .receive()
                .await
                .map(|lmr_req| unsafe {
                    //get `Request` from mr
                    let req = &*(*lmr_req.as_ptr() as *const Request);
                    Self::process_request(req)
                })
                .map_err(|err| println!("{}", &err))
                .unwrap();
            let mut lmr_resp = rdma
                .alloc_local_mr(Layout::new::<Response>())
                .map_err(|err| println!("{}", &err))
                .unwrap();
            // put `Response` into a new mr and send it to client
            unsafe { *(*lmr_resp.as_mut_ptr() as *mut Response) = resp };
            rdma.send(&lmr_resp)
                .await
                .map_err(|err| println!("{}", &err))
                .unwrap();
        }
    }
    /// Process `Add` rpc
    fn add(arg1: &u32, arg2: &u32) -> Response {
        Response::Add { res: *arg1 + *arg2 }
    }

    /// Process request and return the response
    fn process_request(req: &Request) -> Response {
        match req {
            Request::Add { arg1, arg2 } => Self::add(arg1, arg2),
            Request::Sync => Response::Sync,
        }
    }
}

fn transmute_lmr_to_response(lmr: &LocalMr) -> Response {
    unsafe {
        let resp = &*(*lmr.as_ptr() as *const Response);
        match resp {
            Response::Add { res } => Response::Add { res: *res },
            _ => panic!("invalid input : {:?}", resp),
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    //run rpc server
    Server::start("localhost:5555").await;
    println!("rpc server started");
    // //sleep for a second to wait for the server to start
    // tokio::time::sleep(Duration::new(1, 0)).await;
    // let request = Request::Add { arg1: 1, arg2: 1 };
    // let client = Client::new("localhost:5555").await;
    // println!("request: {:?}", request);
    // let res = client.handle_req_sr(request).await;
    // println!("response: {:?}", res);
    // let request = Request::Add { arg1: 2, arg2: 2 };
    // println!("request: {:?}", request);
    // let res = client.handle_req_wr(request).await;
    // println!("response: {:?}", res);
}
