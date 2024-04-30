use crate::service::helloworld::greeter_client::GreeterClient;
use crate::service::helloworld::greeter_server::Greeter;
use crate::service::helloworld::greeter_server::GreeterServer;
use crate::service::helloworld::HelloReply;
use crate::service::helloworld::HelloRequest;
use tonic::{transport::Server, Request, Response, Status};
#[derive(Debug, Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        info!("Got a request: {:?}", request);

        let reply = HelloReply {
            message: format!("Hello {}!", request.into_inner().name),
        };

        Ok(Response::new(reply))
    }
}

pub async fn run_grpc() -> Result<(), anyhow::Error> {
    let addr = "0.0.0.0:50051".parse()?;
    let greeter = MyGreeter::default();
    info!("gRPC server listening on {}", addr);
    Server::builder()
        .add_service(GreeterServer::new(greeter))
        .serve(addr)
        .await?;

    Ok(())
}
