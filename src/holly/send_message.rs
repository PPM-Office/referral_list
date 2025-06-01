use tokio::{io::AsyncWriteExt, net::TcpStream};
use super::Message;

pub async fn send_message(stream: &mut TcpStream, content: String, chat_id: String) -> anyhow::Result<()> {
    let message: Message = Message {
        content,
        chat_id,
        ..Default::default()
    };
    stream.write_all(&message.to_bytes()).await?;
    Ok(())
}