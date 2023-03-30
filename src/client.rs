use async_std::{
    io::{stdin, BufReader},
    net::{TcpStream, ToSocketAddrs},
    prelude::*,
    task,
};
use futures::{select, FutureExt};
use std::process;

// Alias a bunch of things together so we can use a simple `Result`
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/** I bet this runs the client code */
pub fn run(address: String) -> Result<()> {
    task::block_on(client_loop(address))
}

/** We do the work of parsing and sending messages here */
async fn client_loop(addr: impl ToSocketAddrs) -> Result<()> {
    let stream = TcpStream::connect(addr).await?;
    let (reader, mut writer) = (&stream, &stream);
    let mut lines_from_server = BufReader::new(reader).lines().fuse();
    let mut lines_from_stdin = BufReader::new(stdin()).lines().fuse();
    loop {
        select! {
            // Print the incoming message to client
            line = lines_from_server.next().fuse() => match line {
                Some(line) => {
                    let line = line?;
                    println!("{}", line);
                },
                None => break,
            },
            // Send the outgoing lines to server
            line = lines_from_stdin.next().fuse() => match line {
                Some(line) => {
                    let line: String = line?;
                    // TODO these `event_codes` should load from a file or even cooler SurrealDB *flex* 
                    // (for no other reason then to load as much cool stuff into a useless app *wink wink*)
                    match line.as_str() {
                        _ if line == "/exit" => process::exit(0),
                        _ if line.starts_with("/t") => {
                            let parsed_msg: Vec<&str> = line.split_whitespace().collect::<Vec<&str>>();
                            let name = &parsed_msg[1]; // Arg 0 is going to be the /t
                            let msg = &parsed_msg[2];
                            // Format "code|target_peer|msg"
                            let to_server: String = format!("0002|{}|{}", name, msg);
                            writer.write_all(to_server.as_bytes()).await?
                        }
                        _ if line.eq("/online") => writer.write_all("0003|online".as_bytes()).await?,
                        _ => {
                            let to_server = format!("0001|all|{}", line);
                            writer.write_all(to_server.as_bytes()).await?
                        }
                    }
                    writer.write_all(b"\n").await?;
                }
                None => break,
            }
        }
    }
    Ok(())
}
