use async_std::{
    io::BufReader,
    net::{TcpListener, TcpStream, ToSocketAddrs},
    prelude::*,
    task,
};
use futures::channel::mpsc;
use futures::sink::SinkExt;
use futures::{select, FutureExt};
use std::collections::hash_map::{Entry, HashMap};
use std::sync::Arc;

type Sender<T> = mpsc::UnboundedSender<T>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
enum Void {}

#[derive(Debug)]
enum MessageEvent {
    CommandRequest {
        from: String,
    },
    DirectMessage {
        from: String,
        to: Vec<String>,
        msg: String,
    },
    GlobalMessage {
        name: String,
        msg: String,
    },
    NewPeer {
        name: String,
        stream: Arc<TcpStream>,
        shutdown: Receiver<Void>,
    },
}

// FIXME: Currently only used if sending on NewPeer but should be used for ALL sends (this is the clean way!)
/**
 * 5.1: Helper to send messages out based on the message type 
 * This function is in charge of sending messages based on a new connection or existing 
 */
async fn writer_loop(
    messages: &mut Receiver<String>,
    stream: Arc<TcpStream>,
    shutdown: Receiver<Void>,
) -> Result<()> {
    let mut stream = &*stream;
    let mut messages = messages.fuse();
    let mut shutdown = shutdown.fuse();
    loop {
        select! {
            msg = messages.next().fuse() => match msg {
                Some(msg) => stream.write_all(msg.as_bytes()).await?,
                None => break,
            },
            // If an empty message comes back it means we are closing the connection
            void = shutdown.next().fuse() => match void {
                Some(void) => match void {},
                None => break
            }
        }
    }
    Ok(())
}

/**
 * 5: Server Thread Sending
 * This function helps to organize the events into a best effort FIFO queue.
 * (Helps to support graceful shutdown)
 */
async fn broker_loop(events: Receiver<MessageEvent>) {
    let (disconnect_sender, mut disconnect_receiver) = 
        mpsc::unbounded::<(String, Receiver<String>)>();
    let mut peers: HashMap<String, Sender<String>> = HashMap::new();
    let mut events = events.fuse();
    // Match on the targeted event type (See `MessageEvent` enum)
    loop {
        // As a note `select` macro filters on Some / None but drops the thread once future returns None
        let event = select! {
            event = events.next().fuse() => match event {
                None => break, // 2
                Some(e) => e
            },
            // Client is shutting down send any queued messages and then reclaim the resources
            disconnect = disconnect_receiver.next().fuse() => {
                let (name, _pending_messages) = disconnect.unwrap();
                assert!(peers.remove(&name).is_some());
                continue;
            },
        };
        /* This match checks the type of message coming in and does things based on that
         * This means if you want to add more event types add to the enum above and extend the match statment
         * to meat match arm requirments
         */
        match event {
            MessageEvent::DirectMessage { from, to, msg } => {
                for addr in to {
                    if let Some(peer) = peers.get_mut(&addr) {
                        let msg = format!("from {}: {}\n", from, msg);
                        peer.send(msg).await.unwrap()
                    }
                }
            }
            MessageEvent::GlobalMessage { name, msg } => {
                let to_msg: String = format!("from {name}: {msg}\n");
                // Send to everyone
                for (send_to_name, send_handler) in peers.iter_mut() {
                    // Except the current client
                    if &name == send_to_name {
                        let sent_msg: String = format!("sent {msg} to all\n");
                        send_handler.send(sent_msg.clone()).await.unwrap();
                        continue;
                    }
                    send_handler.send(to_msg.clone()).await.unwrap();
                }
            }
            MessageEvent::NewPeer {
                name,
                stream,
                shutdown,
            } => {
                let s: String = format!("Welcome {name} to the chat!\n");
                for send_handler in peers.values_mut() {
                    send_handler.send(s.clone()).await.unwrap();
                }
                match peers.entry(name.clone()) {
                    Entry::Occupied(..) => (),
                    Entry::Vacant(entry) => {
                        let (client_sender, mut client_receiver) = mpsc::unbounded();
                        entry.insert(client_sender);
                        let mut disconnect_sender = disconnect_sender.clone();
                        spawn_and_log_error(async move {
                            let res =
                                writer_loop(&mut client_receiver, stream, shutdown)
                                    .await;
                            disconnect_sender
                                .send((name, client_receiver))
                                .await // 4
                                .unwrap();
                            res
                        });
                    }
                }
            }
            MessageEvent::CommandRequest { from } => {
                let mut online_users: String = String::from("");
                for peer in peers.keys() {
                    let x = format!("{peer}\n");
                    online_users.push_str(&x);
                }
                if let Some(peer) = peers.get_mut(&from) {
                    peer.send(online_users).await.unwrap()
                }
            }
        }
    }
    // Server is shutting down (this thread at least) reclaim the resources explicitly
    drop(peers);
    drop(disconnect_sender);
    // Join the threads together to ensure all cleanup is not just dropped on the floor
    while let Some((_name, _pending_messages)) = disconnect_receiver.next().await {}
}

/** 4: Server Work - Handles messages in / out. Each client connected will have its own thread running this loop for them. */
async fn connection_loop(mut broker: Sender<MessageEvent>, stream: TcpStream) -> Result<()> {
    let stream = Arc::new(stream);
    let reader = BufReader::new(&*stream);
    let mut lines = reader.lines();

    // Initial message shoud be the name they want to be ID'd as
    let name = match lines.next().await {
        None => Err("peer disconnected immediately")?,
        Some(line) => line?,
    };
    let (_shutdown_sender, shutdown_receiver) = mpsc::unbounded::<Void>(); // simple handler for when shutdown occurres
    // The initial message that is sent needs to trigger a MessageEvent::NewPeer to do the initial work of connecting 
    broker
        .send(MessageEvent::NewPeer {
            name: name.clone(),
            stream: Arc::clone(&stream),
            shutdown: shutdown_receiver,
        })
        .await
        .unwrap();

    // Now the thread will listen for new incoming messages.
    // The allowed messages are found in the MessageEvent Enum
    while let Some(line) = lines.next().await {
        let line = line?;
        // FIXME: The loop needs to probably match on a message to test if it fits a given MessageEvent (saves complexity and processing later)
        // We can do better then this but for now check the msg coming in for commands

        // Got a command request (online)
        if line == "/online" {
            broker
                .send(MessageEvent::CommandRequest { from: name.clone() })
                .await
                .unwrap();
            continue;
        }
        // If we find a `:` then user is considered to be sending `MessageEvent::PrivateMessage` otherwise send globally 
        let (dest, msg) = match line.find(':') {
            // FIXME: if we don't find : we assume global message here
            None => {
                broker
                    .send(MessageEvent::GlobalMessage {
                        name: name.clone(),
                        msg: line,
                    })
                    .await
                    .unwrap();
                continue;
            }
            Some(idx) => (&line[..idx], line[idx + 1..].trim()),
        };
        // Split the message from the targeted receiver of the message
        let dest: Vec<String> = dest
            .split(',')
            .map(|name| name.trim().to_string())
            .collect();
        let msg: String = msg.to_string();
        broker
            .send(MessageEvent::DirectMessage {
                from: name.clone(),
                to: dest,
                msg,
            })
            .await
            .unwrap();
    }
    Ok(())
}

/** 3: Create a `Future` watcher - Helper function to log errors when a future creation fails */
fn spawn_and_log_error<F>(fut: F) -> task::JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    task::spawn(async move {
        if let Err(e) = fut.await {
            eprintln!("{}", e)
        }
    })
}

/** 2: Server Accepts connection - Accept incoming connections to the server */
async fn accept_loop(addr: impl ToSocketAddrs) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    let (broker_sender, broker_receiver) = mpsc::unbounded();
    let broker_handle = task::spawn(broker_loop(broker_receiver));
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        println!("Accepting from: {}", stream.peer_addr()?);
        spawn_and_log_error(connection_loop(broker_sender.clone(), stream));
    }
    drop(broker_sender);
    broker_handle.await;
    Ok(())
}

/** 1: Server Starts - IDK about you but I bet this runs the server (similar to main... maybe?) */
pub fn run(network: String) -> Result<()> {
    println!("Server Starting on {}", network);
    let fut = accept_loop(network);
    task::block_on(fut)
}
