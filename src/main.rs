mod client;
mod server;

// My understanding here is that this creates an alias for the right hand side to the left hand
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/** 
 * Start either a client or a server for passing messages
 */
fn main() -> Result<()> {
    let mut args = std::env::args();

    // If we recive a argument of client / server we start that portion otherwise error
    match (args.nth(1).as_ref().map(String::as_str), args.next()) {
        (Some("client"), None) => client::run(),
        (Some("server"), None) => server::run(),
        _ => Err("Usage: rchat [client|server]".into()),
    }
}
