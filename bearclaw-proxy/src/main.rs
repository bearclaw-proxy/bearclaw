mod bootstrap_proxy;

use clap::Parser;

use bootstrap_proxy::{Forwarder, Interceptor};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Bootstrap proxy plugin endpoint to connect to. THIS IS NOT ENCRYPTED!
    #[clap(short, long)]
    bootstrap_proxy_endpoint: String,
}

#[tokio::main]
async fn main() -> bootstrap_proxy::Result<()> {
    console_subscriber::init();

    let args = Args::parse();
    tracing::info!(
        "Bootstrap proxy plugin endpoint: {}\n",
        args.bootstrap_proxy_endpoint
    );

    let mut forwarder = Forwarder::connect(args.bootstrap_proxy_endpoint.as_ref()).await?;
    let mut interceptor = Interceptor::connect(args.bootstrap_proxy_endpoint.as_ref()).await?;

    println!("Sending test request...");

    let response = forwarder
        .forward(
            "example.com",
            443,
            true,
            b"GET / HTTP/1.1\r\nHOST: example.com\r\n\r\n",
        )
        .await?;

    println!("Test response:\n");

    match response {
        Some(r) => println!("{}", String::from_utf8_lossy(&r)),
        None => println!("No response"),
    }

    println!("\nIntercepted messages:\n");

    loop {
        let message = interceptor.intercept().await?;
        println!("-----------------");
        println!("{}", message);
    }
}
