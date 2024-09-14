use bytes::Bytes;
use lol_html::{HtmlRewrite, Settings};
use tokio::sync::mpsc::{self, Receiver, Sender};

pub struct SendHtmlRewrite {
    tx: Sender<Bytes>,
    rx: Receiver<Bytes>,
}

impl SendHtmlRewrite {
    pub fn new() -> Self {
        let (atx, mut arx) = mpsc::channel::<Bytes>(1);
        let (btx, brx) = mpsc::channel::<Bytes>(1);

        tokio::spawn(async move {
            let rewriter = HtmlRewrite::new(Settings::default(), |chunk: &[u8]| {
                let btx = btx.clone();
                let data = Bytes::copy_from_slice(chunk);
                tokio::spawn(async move { btx.send(data).await });
            });

            while let Some(chunk) = arx.recv().await {
                rewriter.write(chunk.as_ref());
            }
        });

        Self { tx: atx, rx: brx }
    }
}
