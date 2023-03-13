use async_std::{
    io::{stdin, BufReader},
    net::{TcpStream, ToSocketAddrs},
    prelude::*,
    task,
};
use futures::{select, FutureExt};

// Alias a bunch of things together so we can use a simple `Result`
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/** I bet this runs the client code */
pub fn run() -> Result<()> {
    task::block_on(client_loop("127.0.0.1:8080"))
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
                    let line = line?;
                    if line == "/clear" {
                        continue;
                    }
                    writer.write_all(line.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                }
                None => break,
            }
        }
    }
    Ok(())
}