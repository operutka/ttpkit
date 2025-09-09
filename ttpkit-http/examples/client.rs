use std::time::Duration;

use ttpkit_http::{
    Body,
    client::{Client, Connector, OutgoingRequest},
};

#[tokio::main]
async fn main() {
    let connector = Connector::new().await.unwrap();

    let request = OutgoingRequest::get("http://google.com")
        .unwrap()
        .body(Body::empty());

    let response = Client::builder()
        .request_timeout(Some(Duration::from_secs(10)))
        .build(connector)
        .request(request)
        .await;

    match response {
        Ok(response) => println!(
            "received HTTP {} {}",
            response.status_code(),
            String::from_utf8_lossy(response.status_message())
        ),
        Err(err) => eprintln!("unable to get HTTP response: {}", err),
    }
}
