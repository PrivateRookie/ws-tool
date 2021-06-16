use log::*;
use ws_client::{errors::WsError, ClientBuilder};

async fn get_case_count() -> Result<u32, WsError> {
    let mut client = ClientBuilder::new("ws://localhost:9001/getCaseCount").build()?;
    client.connect().await?;
    let frame = client.read_frame().await?;
    let mut data = [0u8; 4];
    let unmask_data = frame.payload_data_unmask();
    data[..unmask_data.len()].copy_from_slice(&frame.payload_data_unmask());
    let count = u32::from_be_bytes(data);
    dbg!(client.read_frame().await?);
    client.close().await?;
    Ok(count)
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    pretty_env_logger::init();
    let count = get_case_count().await.unwrap();
    dbg!(count);
    Ok(())
}
