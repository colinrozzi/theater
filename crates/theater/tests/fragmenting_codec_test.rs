use theater::FragmentingCodec;
use tokio_util::codec::{FramedRead, FramedWrite};
use tokio::io::duplex;
use futures::{SinkExt, StreamExt};
use bytes::Bytes;

#[tokio::test]
async fn test_fragmenting_codec_integration() {
    let (client, server) = duplex(64 * 1024 * 1024);
    
    let codec_write = FragmentingCodec::new();
    let codec_read = FragmentingCodec::new();
    
    let mut writer = FramedWrite::new(client, codec_write);
    let mut reader = FramedRead::new(server, codec_read);
    
    // Test small message
    let small_data = b"Hello, World!";
    writer.send(Bytes::from(&small_data[..])).await.unwrap();
    
    // Test large message that requires fragmentation
    let large_data = vec![0xAB; 20 * 1024 * 1024]; // 20MB
    writer.send(Bytes::from(large_data.clone())).await.unwrap();
    
    drop(writer);
    
    // Receive small message
    let received_small = reader.next().await.unwrap().unwrap();
    assert_eq!(received_small.as_ref(), small_data);
    
    // Receive large message
    let received_large = reader.next().await.unwrap().unwrap();
    assert_eq!(received_large.as_ref(), &large_data[..]);
}
