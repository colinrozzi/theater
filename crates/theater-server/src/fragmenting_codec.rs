//! Fixed FragmentingCodec that works properly with Framed::split()

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};
use tracing::{debug, warn};

/// Maximum size for a single fragment data (12MB)
/// This leaves room for JSON serialization overhead while staying well under the 32MB frame limit
const MAX_FRAGMENT_DATA_SIZE: usize = 8 * 1024 * 1024; // Reduced to 8MB to account for base64 + JSON overhead

/// How long to keep partial messages before timing out (30 seconds)
const FRAGMENT_TIMEOUT: Duration = Duration::from_secs(30);

/// A single fragment of a larger message
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Fragment {
    /// Unique identifier for the complete message
    message_id: u64,
    /// Index of this fragment (0-based)
    fragment_index: u32,
    /// Total number of fragments for this message
    total_fragments: u32,
    /// The actual data chunk (base64 encoded for efficient JSON serialization)
    data: String,
}

/// Internal wrapper to distinguish between complete messages and fragments
#[derive(Debug, Clone, Serialize, Deserialize)]
enum FrameType {
    /// A complete message that doesn't need fragmentation
    Complete(Vec<u8>),
    /// A fragment of a larger message
    Fragment(Fragment),
}

/// Partial message being reassembled
#[derive(Debug)]
struct PartialMessage {
    /// When this partial message was first created
    created_at: Instant,
    /// Total number of fragments expected
    total_fragments: u32,
    /// Fragments received so far, indexed by fragment_index
    fragments: HashMap<u32, Vec<u8>>,
}

impl PartialMessage {
    fn new(total_fragments: u32) -> Self {
        Self {
            created_at: Instant::now(),
            total_fragments,
            fragments: HashMap::new(),
        }
    }

    fn add_fragment(&mut self, index: u32, data: Vec<u8>) {
        self.fragments.insert(index, data);
    }

    fn is_complete(&self) -> bool {
        self.fragments.len() == self.total_fragments as usize
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > FRAGMENT_TIMEOUT
    }

    fn reassemble(self) -> io::Result<Vec<u8>> {
        if !self.is_complete() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Cannot reassemble incomplete message",
            ));
        }

        let mut result = Vec::new();

        // Reassemble fragments in order
        for i in 0..self.total_fragments {
            if let Some(fragment_data) = self.fragments.get(&i) {
                result.extend_from_slice(fragment_data);
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Missing fragment {}", i),
                ));
            }
        }

        Ok(result)
    }
}

/// Shared state between encoder and decoder
#[derive(Debug)]
struct SharedState {
    /// Counter for generating unique message IDs
    next_message_id: AtomicU64,
    /// Partial messages being reassembled (keyed by message_id)
    partial_messages: Mutex<HashMap<u64, PartialMessage>>,
    /// Last time we cleaned up expired partial messages
    last_cleanup: Mutex<Instant>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            next_message_id: AtomicU64::new(1),
            partial_messages: Mutex::new(HashMap::new()),
            last_cleanup: Mutex::new(Instant::now()),
        }
    }

    fn next_message_id(&self) -> u64 {
        self.next_message_id.fetch_add(1, Ordering::Relaxed)
    }

    fn cleanup_expired(&self) {
        // Only cleanup periodically to avoid overhead
        {
            let last_cleanup = self.last_cleanup.lock().unwrap();
            if last_cleanup.elapsed() < Duration::from_secs(10) {
                return;
            }
        }

        let mut partial_messages = self.partial_messages.lock().unwrap();
        let before_count = partial_messages.len();

        partial_messages.retain(|message_id, partial| {
            if partial.is_expired() {
                warn!("Cleaning up expired partial message {}", message_id);
                false
            } else {
                true
            }
        });

        let cleaned = before_count - partial_messages.len();
        if cleaned > 0 {
            debug!("Cleaned up {} expired partial messages", cleaned);
        }

        *self.last_cleanup.lock().unwrap() = Instant::now();
    }
}

/// A codec that transparently handles message fragmentation
#[derive(Debug)]
pub struct FragmentingCodec {
    /// Underlying length-delimited codec
    inner: LengthDelimitedCodec,
    /// Shared state between encoder and decoder
    shared_state: Arc<SharedState>,
}

impl FragmentingCodec {
    /// Create a new fragmenting codec with the same configuration as Theater's current setup
    pub fn new() -> Self {
        let mut inner = LengthDelimitedCodec::new();
        inner.set_max_frame_length(32 * 1024 * 1024); // 32MB max frame

        Self {
            inner,
            shared_state: Arc::new(SharedState::new()),
        }
    }

    /// Fragment a large message into chunks
    fn fragment_message(&self, data: &[u8]) -> Vec<Fragment> {
        let message_id = self.shared_state.next_message_id();
        let total_size = data.len();

        // Use the defined chunk size constant
        let chunk_size = MAX_FRAGMENT_DATA_SIZE;

        // Calculate how many fragments we need
        let total_fragments = (total_size + chunk_size - 1) / chunk_size;

        debug!(
            "Fragmenting message {} into {} fragments (total size: {} bytes, chunk size: {} bytes)",
            message_id, total_fragments, total_size, chunk_size
        );

        let mut fragments = Vec::new();

        for (i, chunk) in data.chunks(chunk_size).enumerate() {
            let fragment = Fragment {
                message_id,
                fragment_index: i as u32,
                total_fragments: total_fragments as u32,
                data: BASE64.encode(chunk),
            };

            // Debug: check serialized size to ensure it's under the frame limit
            if let Ok(serialized) = serde_json::to_vec(&FrameType::Fragment(fragment.clone())) {
                debug!("Fragment {} serialized size: {} bytes", i, serialized.len());
                if serialized.len() > 31 * 1024 * 1024 {
                    // Close to 32MB limit
                    warn!("Fragment {} serialized size ({} bytes) is dangerously close to frame limit", i, serialized.len());
                }
            }

            fragments.push(fragment);
        }

        fragments
    }
}

impl Default for FragmentingCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for FragmentingCodec {
    fn clone(&self) -> Self {
        let mut inner = LengthDelimitedCodec::new();
        inner.set_max_frame_length(32 * 1024 * 1024); // 32MB max frame - CRITICAL!

        Self {
            inner,
            shared_state: Arc::clone(&self.shared_state),
        }
    }
}

impl Encoder<Bytes> for FragmentingCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let data = item.to_vec();

        // Check if we need to fragment this message
        if data.len() <= MAX_FRAGMENT_DATA_SIZE {
            // Small message - send as complete
            let frame = FrameType::Complete(data);
            let serialized = serde_json::to_vec(&frame).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to serialize frame: {}", e),
                )
            })?;

            self.inner.encode(Bytes::from(serialized), dst)
        } else {
            // Large message - fragment it
            let fragments = self.fragment_message(&data);

            // Encode each fragment into the destination buffer
            for fragment in fragments {
                let frame = FrameType::Fragment(fragment);
                let serialized = serde_json::to_vec(&frame).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Failed to serialize fragment: {}", e),
                    )
                })?;

                // Create a temporary buffer for this fragment
                let mut fragment_buf = BytesMut::new();
                self.inner
                    .encode(Bytes::from(serialized), &mut fragment_buf)?;

                // Append to the main destination buffer
                dst.extend_from_slice(&fragment_buf);
            }

            Ok(())
        }
    }
}

impl Decoder for FragmentingCodec {
    type Item = Bytes;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Clean up expired messages periodically
        self.shared_state.cleanup_expired();

        // Try to decode a frame from the underlying codec
        if let Some(frame_bytes) = self.inner.decode(src)? {
            // Deserialize the frame
            let frame: FrameType = serde_json::from_slice(&frame_bytes).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to deserialize frame: {}", e),
                )
            })?;

            match frame {
                FrameType::Complete(data) => {
                    // Complete message - return immediately
                    Ok(Some(Bytes::from(data)))
                }
                FrameType::Fragment(fragment) => {
                    // Fragment - add to partial message
                    let message_id = fragment.message_id;
                    let fragment_index = fragment.fragment_index;
                    let total_fragments = fragment.total_fragments;

                    debug!(
                        "Received fragment {}/{} for message {}",
                        fragment_index + 1,
                        total_fragments,
                        message_id
                    );

                    // Decode the base64 data
                    let fragment_data = BASE64.decode(&fragment.data).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Failed to decode fragment data: {}", e),
                        )
                    })?;

                    // Get or create partial message
                    let mut partial_messages = self.shared_state.partial_messages.lock().unwrap();
                    let partial = partial_messages
                        .entry(message_id)
                        .or_insert_with(|| PartialMessage::new(total_fragments));

                    // Add this fragment
                    partial.add_fragment(fragment_index, fragment_data);

                    // Check if message is complete
                    if partial.is_complete() {
                        debug!("Message {} is complete, reassembling", message_id);

                        // Remove from partial messages and reassemble
                        let partial = partial_messages.remove(&message_id).unwrap();
                        drop(partial_messages); // Release the lock

                        let complete_data = partial.reassemble()?;
                        Ok(Some(Bytes::from(complete_data)))
                    } else {
                        // Still waiting for more fragments
                        debug!(
                            "Message {} still incomplete ({}/{} fragments)",
                            message_id,
                            partial.fragments.len(),
                            total_fragments
                        );
                        Ok(None)
                    }
                }
            }
        } else {
            // No complete frame available yet
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use tokio::io::duplex;
    use tokio_util::codec::{FramedRead, FramedWrite};

    #[tokio::test]
    async fn test_small_message_no_fragmentation() {
        let (client, server) = duplex(1024);

        let codec_write = FragmentingCodec::new();
        let codec_read = FragmentingCodec::new();

        let mut writer = FramedWrite::new(client, codec_write);
        let mut reader = FramedRead::new(server, codec_read);

        let test_data = b"Hello, World!";

        // Send small message
        writer.send(Bytes::from(&test_data[..])).await.unwrap();
        drop(writer); // Close writer

        // Receive should get the same data
        let received = reader.next().await.unwrap().unwrap();
        assert_eq!(received.as_ref(), test_data);
    }

    #[tokio::test]
    async fn test_large_message_fragmentation() {
        let (client, server) = duplex(64 * 1024 * 1024); // Large buffer

        let codec_write = FragmentingCodec::new();
        let codec_read = FragmentingCodec::new();

        let mut writer = FramedWrite::new(client, codec_write);
        let mut reader = FramedRead::new(server, codec_read);

        // Create a message larger than MAX_FRAGMENT_DATA_SIZE
        let test_data = vec![0xAB; MAX_FRAGMENT_DATA_SIZE + 1000];

        // Send large message
        match writer.send(Bytes::from(test_data.clone())).await {
            Ok(_) => println!("Successfully sent large message"),
            Err(e) => {
                println!("Error sending: {:?}", e);
                panic!("Failed to send: {}", e);
            }
        }
        drop(writer); // Close writer

        // Receive should get the same data
        let received = reader.next().await.unwrap().unwrap();
        assert_eq!(received.as_ref(), &test_data[..]);
    }

    #[test]
    fn test_fragment_message() {
        let codec = FragmentingCodec::new();
        let data = vec![0x42; MAX_FRAGMENT_DATA_SIZE + 500];

        let fragments = codec.fragment_message(&data);

        assert_eq!(fragments.len(), 2);
        assert_eq!(fragments[0].fragment_index, 0);
        assert_eq!(fragments[1].fragment_index, 1);
        assert_eq!(fragments[0].total_fragments, 2);
        assert_eq!(fragments[1].total_fragments, 2);
        assert_eq!(fragments[0].message_id, fragments[1].message_id);

        // Check data integrity
        let mut reassembled = Vec::new();
        let decoded_0 = BASE64.decode(&fragments[0].data).unwrap();
        let decoded_1 = BASE64.decode(&fragments[1].data).unwrap();
        reassembled.extend_from_slice(&decoded_0);
        reassembled.extend_from_slice(&decoded_1);
        assert_eq!(reassembled, data);
    }

    #[test]
    fn test_partial_message_assembly() {
        let mut partial = PartialMessage::new(3);

        assert!(!partial.is_complete());

        partial.add_fragment(0, vec![1, 2, 3]);
        partial.add_fragment(2, vec![7, 8, 9]);
        assert!(!partial.is_complete());

        partial.add_fragment(1, vec![4, 5, 6]);
        assert!(partial.is_complete());

        let reassembled = partial.reassemble().unwrap();
        assert_eq!(reassembled, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
