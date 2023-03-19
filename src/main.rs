mod client;
mod server;

use clap::Parser;
// My understanding here is that this creates an alias for the right hand side to the left hand
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Default, Parser)]
#[command(author, version, about="A super simple chat server / client")]
struct Args {
    /// The run mode of the application
    #[arg(help_heading = "Arguments", value_name = "client|server", short, long)]
    mode: String,

    /// Port to connect on
    #[arg(help_heading = "Arguments", short, long)]
    port: String,

    /// Host
    #[arg(help_heading = "Arguments", short = 'a', long = "Host address")]
    host: String,
}

/**
 * Start either a client or a server for passing messages
 */
fn main() -> Result<()> {
    // let mut args = std::env::args();
    let args = Args::parse();
    let network_address = format!("{}:{}", args.host, args.port);

    // If we recive a argument of client / server we start that portion otherwise error
    match args.mode.as_str() {
        "client" => client::run(network_address),
        "server" => server::run(network_address),
        _ => Err("Usage: rchat [client|server]".into()),
    }
}
