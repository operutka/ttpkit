use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use tokio::net::TcpListener;
use ttpkit_http::{
    Body, Error,
    response::Status,
    server::{IncomingRequest, OutgoingResponse, RequestHandler, Server},
};

#[derive(Copy, Clone)]
struct SimpleHandler;

impl RequestHandler for SimpleHandler {
    async fn try_handle_request(&self, _: IncomingRequest) -> Result<OutgoingResponse, Error> {
        let res = OutgoingResponse::builder()
            .set_status(Status::OK)
            .add_header_field(("Content-Type", "text/plain"))
            .body(Body::from("Hello, World!"));

        Ok(res)
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080)))
        .await
        .unwrap();

    Server::builder()
        .request_header_timeout(Some(Duration::from_secs(20)))
        .build(listener)
        .serve(SimpleHandler)
        .await
        .unwrap();
}
