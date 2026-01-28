use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;

pub struct LspTransport {
    stdin: ChildStdin,
    stdout: ChildStdout,
}

pub struct LspWriter {
    stdin: ChildStdin,
}

pub struct LspReader {
    stdout: ChildStdout,
}

impl LspTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self { stdin, stdout }
    }

    pub fn split(self) -> (LspWriter, LspReader) {
        (
            LspWriter { stdin: self.stdin },
            LspReader {
                stdout: self.stdout,
            },
        )
    }
}

impl LspWriter {
    pub async fn write_message(&mut self, json_text: &str) -> Result<()> {
        let bytes = json_text.as_bytes();
        let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
        self.stdin.write_all(header.as_bytes()).await?;
        self.stdin.write_all(bytes).await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

impl LspReader {
    pub async fn read_message(&mut self) -> Result<String> {
        let mut header_buf: Vec<u8> = Vec::new();
        loop {
            let mut byte = [0u8];
            let n = self.stdout.read(&mut byte).await?;
            if n == 0 {
                return Err(anyhow!("LSP stdout closed"));
            }
            header_buf.push(byte[0]);
            if header_buf.ends_with(b"\r\n\r\n") {
                break;
            }
            if header_buf.len() > 16 * 1024 {
                return Err(anyhow!("LSP header too large"));
            }
        }

        let header = String::from_utf8_lossy(&header_buf);
        let mut content_len: Option<usize> = None;
        for line in header.lines() {
            if let Some(rest) = line.strip_prefix("Content-Length:") {
                let len = rest.trim().parse::<usize>().context("bad Content-Length")?;
                content_len = Some(len);
            }
        }
        let len = content_len.ok_or_else(|| anyhow!("missing Content-Length"))?;
        let mut body = vec![0u8; len];
        self.stdout.read_exact(&mut body).await?;
        let payload = String::from_utf8(body).context("LSP body not utf-8")?;
        Ok(payload)
    }
}
