extern crate tinyfiledialogs;

use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, SystemTime};


#[derive(Debug, PartialEq)]
enum MainControlMsg {
    RoundTrip(Duration),
    IncomingMessage(String),
    ClientDisconnected,
    ServerShutDown
}

enum ComponentControlMsg {
    OutgoingMessage(String),
    Quit
}

// TODO: implement a proper codec.
// Currently assuming messages are < 24 bytes, and padding them.
// Also assuming ACK message is 3 bytes.
const EMPTY_MESSAGE: &'static str = "\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}";

fn time_roundtrip<F: FnMut()>(mut f: F) -> Duration {
    let sys_time = SystemTime::now();
    f();
    sys_time.elapsed().unwrap()
}

fn acknowledge_receipt(stream: &mut TcpStream) {
    let _ = stream.write("ACK".as_bytes());
    stream.flush().unwrap();
}

fn wait_for_ack(stream: &mut TcpStream) {
    let mut buffer = [0; 3];
    let _ = stream.read(&mut buffer);
}

fn send_chat(stream: &mut TcpStream, chat: &str) {
    let _ = stream.write(chat.as_bytes());
    stream.flush().unwrap();
}

fn wait_for_message(stream: &mut TcpStream,
                    main_chan: &Sender<MainControlMsg>)
                    -> bool {
    let mut buffer = [0; 24];
    let _ = stream.read(&mut buffer);
    let message = String::from_utf8_lossy(&buffer[..]);
    if message == EMPTY_MESSAGE {
        // Peer disconnected
        return false;
    }
    acknowledge_receipt(stream);
    let _ = main_chan.send(MainControlMsg::IncomingMessage(message.to_string()));
    true
}

fn wait_for_input(stream: &mut TcpStream,
                main_chan: &Sender<MainControlMsg>,
                port: &Receiver<ComponentControlMsg>)
                -> bool {
    let control_msg = match port.recv() {
        Err(_) => return false,
        Ok(control_msg) => control_msg,
    };
    let chat: String = match control_msg {
        ComponentControlMsg::OutgoingMessage(chat) => chat,
        ComponentControlMsg::Quit => return false,
    };
    let duration = time_roundtrip(|| {
        send_chat(stream, chat.as_str());
        wait_for_ack(stream);
    });
    let _ = main_chan.send(MainControlMsg::RoundTrip(duration));
    true
}

fn start_server(main_chan: Sender<MainControlMsg>) -> Sender<ComponentControlMsg> {
    let (chan, port) = channel();
    let _ = thread::Builder::new().spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:8000").unwrap();
        let mut keep_accepting = true;
        while keep_accepting {
            let client = listener.accept();
            if let Ok((mut stream, _)) = client {
                let handshake = "Lets chat!!";
                send_chat(&mut stream, &handshake);
                // Handle the first ACK from client...
                wait_for_ack(&mut stream);
                loop {
                    if !wait_for_message(&mut stream, &main_chan) {
                        // Client disconnect, break out of the loop,
                        // and start accepting the next one.
                        break;
                    }
                    keep_accepting = wait_for_input(&mut stream, &main_chan, &port);
                    if !keep_accepting {
                        // Server shutdown.
                        break;
                    }
                }
            }
        }
        let _ = main_chan.send(MainControlMsg::ServerShutDown);
    });
    chan
}

fn start_client(main_chan: Sender<MainControlMsg>) -> Sender<ComponentControlMsg> {
    let (chan, port) = channel();
    let _ = thread::Builder::new().spawn(move || {
        let mut stream = TcpStream::connect("127.0.0.1:8000").expect("please start server first");
        loop {
            if !wait_for_message(&mut stream, &main_chan) {
                 // Client disconnects when server is gone.
                break;
            }
            if !wait_for_input(&mut stream, &main_chan, &port) {
                // Client also disconnects in responses to a Quit message.
                break;
            }
        }
        let _ = main_chan.send(MainControlMsg::ClientDisconnected);
    });
    chan
}

fn main() {
    let mut arguments = env::args();
    let _ = arguments.next();
    let server_or_client = arguments.next().unwrap();
    let (chan, port) = channel();
    let (component, peer_name) = match server_or_client.as_ref() {
        "server" => (start_server(chan), "client"),
        "client" => (start_client(chan), "server"),
        _ => panic!("unknown argument - usage is 'cargo run -- {server|client}")
    };
    loop {
        let incoming = match port.try_recv() {
            Err(_) => continue,
            Ok(incoming) => incoming,
        };
        let received = match incoming {
            MainControlMsg::IncomingMessage(received) => received,
            MainControlMsg::RoundTrip(duration) => {
                println!("Roundtrip took: {:?}", duration);
                continue
            },
            MainControlMsg::ClientDisconnected => {
                assert_eq!(server_or_client, "client");
                print!("No server available, quitting");
                break;
            },
            MainControlMsg::ServerShutDown => {
                assert_eq!(server_or_client, "server");
                print!("Server has gone away");
                break;
            },
        };
        println!("{:?} received: {:?}", server_or_client, received);
        let title = format!("Simple chat {} - Choose 'Cancel' to quit", server_or_client);
        let prompt = format!("Send message to {}", peer_name);
        match tinyfiledialogs::input_box(&title, &prompt, &"") {
            Some(input) => {
                let _ = component.send(ComponentControlMsg::OutgoingMessage(input));
            },
            None => {
                println!("{:?} quitting", server_or_client);
                let _ = component.send(ComponentControlMsg::Quit);
                break;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_server_and_client_messaging() {
        let (server_chan, server_port) = channel();
        let (client_chan, client_port) = channel();
        let server = start_server(server_chan);
        // Ensure the server has had time to start.
        sleep(Duration::new(1, 0));
        let client = start_client(client_chan.clone());
        let mut server_msgs = server_port.iter();
        let mut client_msgs = client_port.iter();
        assert!(client_msgs.next().is_some());

        // Send a message to the server, via the client component.
        let _ = client.send(ComponentControlMsg::OutgoingMessage("test one".to_string()));
        let from_client_message = "test one\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}".to_string();
        assert_eq!(server_msgs.next().unwrap(), MainControlMsg::IncomingMessage(from_client_message));
        // Check that we got the roundtrip message from the client component.
        let mut roundtrip = false;
        if let Some(MainControlMsg::RoundTrip(_)) = client_msgs.next() {
            roundtrip = true;
        }
        assert!(roundtrip);

        // Send a message to the client, via the server.
        let _ = server.send(ComponentControlMsg::OutgoingMessage("test two".to_string()));
        let from_server_message = "test two\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}".to_string();
        assert_eq!(client_msgs.next().unwrap(), MainControlMsg::IncomingMessage(from_server_message));
        // Check that we got the roundtrip message from the server component.
        let mut server_roundtrip = false;
        if let Some(MainControlMsg::RoundTrip(_)) = server_msgs.next() {
            server_roundtrip = true;
        }
        assert!(server_roundtrip);

        // Disconnect the client.
        let _ = client.send(ComponentControlMsg::Quit);
        // Check that the client disconnects
        let disconnect = client_msgs.next().unwrap();
        assert_eq!(MainControlMsg::ClientDisconnected, disconnect);

        // Start a new client.
        let client_2 = start_client(client_chan);
        // Check that we got the "let's chat" handshake from the server.
        assert!(client_msgs.next().is_some());

        // Send a message to the server, via the new client component.
        let _ = client_2.send(ComponentControlMsg::OutgoingMessage("test three".to_string()));
        let from_client_2_message = "test three\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}".to_string();
        assert_eq!(server_msgs.next().unwrap(), MainControlMsg::IncomingMessage(from_client_2_message));

        // Check that we got the roundtrip message from the client component.
        let mut roundtrip_2 = false;
        if let Some(MainControlMsg::RoundTrip(_)) = client_msgs.next() {
            roundtrip_2 = true;
        }
        assert!(roundtrip_2);

        // Cleaning up.
        let _ = server.send(ComponentControlMsg::Quit);
        // Check that the server shuts down.
        let disconnect = server_msgs.next().unwrap();
        assert_eq!(MainControlMsg::ServerShutDown, disconnect);

        // Check that the client disconnects when the server is gone.
        let disconnect = client_msgs.next().unwrap();
        assert_eq!(MainControlMsg::ClientDisconnected, disconnect);
    }
}
