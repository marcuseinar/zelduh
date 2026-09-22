//! A signalling relay: the optional half of multiplayer.
//!
//! Players find each other through public infrastructure by default and then
//! talk directly, so this server is not needed to play. It exists for the two
//! cases where public matchmaking will not do: a network with no way out to
//! the internet, and anyone who would rather their room code were not handed
//! to a stranger's relay.
//!
//! It speaks Trystero's `ws-relay` protocol, which is topic pub/sub and
//! nothing more. A client subscribes to a topic derived from the room code
//! and publishes offers and answers to it; this relay copies each message to
//! whoever else is listening. Once the peers have swapped those, WebRTC takes
//! over and no further game traffic comes anywhere near here.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use crate::json;

/// Topics one connection may hold at once. A room needs a handful.
const MAX_TOPICS_PER_CLIENT: usize = 64;
/// How long a topic name may be.
const MAX_TOPIC_LEN: usize = 256;

/// A connected client.
struct Client {
    out: Sender<String>,
    topics: Vec<String>,
}

/// Every connection and who is listening to what.
#[derive(Default)]
pub struct Relay {
    clients: HashMap<usize, Client>,
    /// Topic to the clients subscribed to it.
    topics: HashMap<String, Vec<usize>>,
    next_id: usize,
}

impl Relay {
    pub fn new() -> Relay {
        Relay::default()
    }

    /// Registers a connection and returns the handle used to refer to it.
    pub fn connect(&mut self, out: Sender<String>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.clients.insert(
            id,
            Client {
                out,
                topics: Vec::new(),
            },
        );
        id
    }

    /// Forgets a connection and every topic it was listening to.
    pub fn disconnect(&mut self, id: usize) {
        let Some(client) = self.clients.remove(&id) else {
            return;
        };
        for topic in client.topics {
            if let Some(list) = self.topics.get_mut(&topic) {
                list.retain(|c| *c != id);
                if list.is_empty() {
                    self.topics.remove(&topic);
                }
            }
        }
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    pub fn topic_count(&self) -> usize {
        self.topics.len()
    }

    /// Handles one message from a client. Returns false when it made no sense.
    pub fn handle(&mut self, id: usize, text: &str) -> bool {
        let Some(kind) = json::string_member(text, "type") else {
            return false;
        };
        let Some(topic) = json::string_member(text, "topic") else {
            return false;
        };
        if topic.is_empty() || topic.len() > MAX_TOPIC_LEN {
            return false;
        }
        match kind.as_str() {
            "subscribe" => self.subscribe(id, topic),
            "unsubscribe" => self.unsubscribe(id, &topic),
            "publish" => {
                let Some(payload) = json::member(text, "payload") else {
                    return false;
                };
                self.publish(&topic, payload);
                true
            }
            _ => false,
        }
    }

    fn subscribe(&mut self, id: usize, topic: String) -> bool {
        let Some(client) = self.clients.get_mut(&id) else {
            return false;
        };
        if client.topics.contains(&topic) {
            return true;
        }
        if client.topics.len() >= MAX_TOPICS_PER_CLIENT {
            return false;
        }
        client.topics.push(topic.clone());
        self.topics.entry(topic).or_default().push(id);
        true
    }

    fn unsubscribe(&mut self, id: usize, topic: &str) -> bool {
        if let Some(client) = self.clients.get_mut(&id) {
            client.topics.retain(|t| t != topic);
        }
        if let Some(list) = self.topics.get_mut(topic) {
            list.retain(|c| *c != id);
            if list.is_empty() {
                self.topics.remove(topic);
            }
        }
        true
    }

    /// Copies a message to everyone listening, sender included.
    ///
    /// The sender gets its own message back because that is what the public
    /// relays this protocol was written against do, and Trystero is built to
    /// recognise and drop its own traffic.
    fn publish(&mut self, topic: &str, payload: &str) {
        let Some(listeners) = self.topics.get(topic) else {
            return;
        };
        let message = format!(
            "{{\"topic\":{},\"payload\":{}}}",
            json::quote(topic),
            payload
        );
        let mut gone = Vec::new();
        for id in listeners {
            let Some(client) = self.clients.get(id) else {
                continue;
            };
            if client.out.send(message.clone()).is_err() {
                gone.push(*id);
            }
        }
        for id in gone {
            self.disconnect(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver};

    fn client(relay: &mut Relay) -> (usize, Receiver<String>) {
        let (tx, rx) = channel();
        (relay.connect(tx), rx)
    }

    #[test]
    fn a_message_reaches_everyone_listening() {
        let mut relay = Relay::new();
        let (a, a_rx) = client(&mut relay);
        let (b, b_rx) = client(&mut relay);
        let (c, c_rx) = client(&mut relay);
        assert!(relay.handle(a, r#"{"type":"subscribe","topic":"room"}"#));
        assert!(relay.handle(b, r#"{"type":"subscribe","topic":"room"}"#));
        assert!(relay.handle(c, r#"{"type":"subscribe","topic":"elsewhere"}"#));

        assert!(relay.handle(a, r#"{"type":"publish","topic":"room","payload":{"o":1}}"#));
        let expected = r#"{"topic":"room","payload":{"o":1}}"#;
        assert_eq!(
            a_rx.try_recv().as_deref(),
            Ok(expected),
            "including the sender"
        );
        assert_eq!(b_rx.try_recv().as_deref(), Ok(expected));
        assert!(c_rx.try_recv().is_err(), "another topic hears nothing");
    }

    #[test]
    fn leaving_a_topic_stops_the_messages() {
        let mut relay = Relay::new();
        let (a, _a_rx) = client(&mut relay);
        let (b, b_rx) = client(&mut relay);
        relay.handle(a, r#"{"type":"subscribe","topic":"room"}"#);
        relay.handle(b, r#"{"type":"subscribe","topic":"room"}"#);
        relay.handle(b, r#"{"type":"unsubscribe","topic":"room"}"#);
        relay.handle(a, r#"{"type":"publish","topic":"room","payload":"x"}"#);
        assert!(b_rx.try_recv().is_err());
        assert_eq!(relay.topic_count(), 1, "the sender is still listening");
    }

    #[test]
    fn a_disconnect_cleans_up_after_itself() {
        let mut relay = Relay::new();
        let (a, _rx) = client(&mut relay);
        relay.handle(a, r#"{"type":"subscribe","topic":"one"}"#);
        relay.handle(a, r#"{"type":"subscribe","topic":"two"}"#);
        assert_eq!(relay.topic_count(), 2);
        relay.disconnect(a);
        assert_eq!(relay.topic_count(), 0);
        assert_eq!(relay.client_count(), 0);
        // A message for a topic nobody holds is simply dropped.
        relay.handle(a, r#"{"type":"publish","topic":"one","payload":1}"#);
    }

    #[test]
    fn a_dead_socket_is_dropped_on_the_next_message() {
        let mut relay = Relay::new();
        let (a, a_rx) = client(&mut relay);
        let (b, b_rx) = client(&mut relay);
        relay.handle(a, r#"{"type":"subscribe","topic":"room"}"#);
        relay.handle(b, r#"{"type":"subscribe","topic":"room"}"#);
        drop(b_rx);
        relay.handle(a, r#"{"type":"publish","topic":"room","payload":1}"#);
        assert!(a_rx.try_recv().is_ok());
        assert_eq!(relay.client_count(), 1, "the closed one was forgotten");
    }

    #[test]
    fn nonsense_is_refused_quietly() {
        let mut relay = Relay::new();
        let (a, _rx) = client(&mut relay);
        for bad in [
            "",
            "{}",
            "not json",
            r#"{"type":"subscribe"}"#,
            r#"{"topic":"room"}"#,
            r#"{"type":"shout","topic":"room"}"#,
            r#"{"type":"publish","topic":"room"}"#,
            r#"{"type":"subscribe","topic":""}"#,
        ] {
            assert!(!relay.handle(a, bad), "{bad}");
        }
        assert_eq!(relay.topic_count(), 0);
    }

    #[test]
    fn one_client_cannot_hoard_topics() {
        let mut relay = Relay::new();
        let (a, _rx) = client(&mut relay);
        for i in 0..MAX_TOPICS_PER_CLIENT {
            assert!(relay.handle(a, &format!(r#"{{"type":"subscribe","topic":"t{i}"}}"#)));
        }
        assert!(!relay.handle(a, r#"{"type":"subscribe","topic":"one-too-many"}"#));
        assert_eq!(relay.topic_count(), MAX_TOPICS_PER_CLIENT);
    }
}
