use futures::io::{AsyncRead, AsyncSeek, SeekFrom};
use futures::stream::Stream;
use futures::stream::StreamExt;
use std::future::Future;
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::{HypercoreEvent, RemoteHypercore};

type GetOutput = Result<Option<Vec<u8>>>;
type GetFuture = Pin<Box<dyn Future<Output = (RemoteHypercore, GetOutput)> + Send>>;

type SeekOutput = Result<(u64, usize)>;
type SeekFuture = Pin<Box<dyn Future<Output = (RemoteHypercore, SeekOutput)> + Send>>;

async fn get(mut core: RemoteHypercore, seq: u64) -> (RemoteHypercore, GetOutput) {
    let block = core.get(seq).await;
    (core, block)
}

async fn seek(mut core: RemoteHypercore, byte_offset: u64) -> (RemoteHypercore, SeekOutput) {
    let result = core.seek(byte_offset).await;
    (core, result)
}

async fn onappend_get(mut core: RemoteHypercore, seq: u64) -> (RemoteHypercore, GetOutput) {
    let mut events = core.subscribe();
    while let Some(event) = events.next().await {
        match event {
            HypercoreEvent::Append => {
                return get(core, seq).await;
            }
            HypercoreEvent::Download(downloaded_seq) if downloaded_seq == seq => {
                return get(core, seq).await;
            }
            _ => {}
        }
    }
    (core, Ok(None))
}

fn get_future(core: RemoteHypercore, seq: u64) -> GetFuture {
    Box::pin(get(core, seq))
}

fn onappend_future(core: RemoteHypercore, seq: u64) -> GetFuture {
    Box::pin(onappend_get(core, seq))
}

fn seek_future(core: RemoteHypercore, byte_offset: usize) -> SeekFuture {
    Box::pin(seek(core, byte_offset as u64))
}

pub struct ReadStream {
    future: GetFuture,
    index: u64,
    end: Option<u64>,
    live: bool,
    finished: bool,
}

impl ReadStream {
    pub fn new(core: RemoteHypercore, start: u64, end: Option<u64>, live: bool) -> Self {
        Self {
            future: get_future(core, start),
            index: start,
            end,
            live,
            finished: false,
        }
    }
}

impl Stream for ReadStream {
    type Item = Result<Vec<u8>>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }

        let poll_result = Pin::as_mut(&mut self.future).poll(cx);
        let (core, result) = futures::ready!(poll_result);
        let result = result.transpose();

        let len = self.end.unwrap_or_else(|| core.len());
        self.index += 1;
        if len == self.index && !self.live {
            self.finished = true;
        } else if len == self.index {
            self.future = onappend_future(core, self.index);
        } else {
            self.future = get_future(core, self.index)
        }
        Poll::Ready(result)
    }
}

pub struct ByteStream {
    core: RemoteHypercore,
    seq: u64,
    block_offset: usize,
    byte_offset: usize,
    state: State,
}

impl ByteStream {
    pub fn new(core: RemoteHypercore) -> Self {
        Self {
            core,
            seq: 0,
            block_offset: 0,
            byte_offset: 0,
            state: State::Idle,
        }
    }
}

enum State {
    Idle,
    Seeking(SeekFuture),
    Reading(GetFuture),
    Ready { block: Vec<u8> },
    Finished,
}
impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Customize so only `x` and `y` are denoted.
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Seeking(_) => write!(f, "Seeking"),
            Self::Reading(_) => write!(f, "Reading"),
            Self::Ready { block } => write!(f, "Ready {}", block.len()),
            Self::Finished => write!(f, "Finished"),
        }
    }
}

impl AsyncSeek for ByteStream {
    fn poll_seek(mut self: Pin<&mut Self>, cx: &mut Context, pos: SeekFrom) -> Poll<Result<u64>> {
        let mut seq = self.seq;
        let mut byte_offset = self.byte_offset;
        let mut block_offset = self.block_offset;
        let mut finished = false;
        while !finished {
            self.state = match &mut self.state {
                State::Seeking(future) => {
                    let poll_result = Pin::as_mut(future).poll(cx);
                    let (_, block) = futures::ready!(poll_result);
                    // TODO: Handle error.
                    let (res_seq, res_offset) = block.unwrap();
                    seq = res_seq;
                    block_offset = res_offset;
                    finished = true;
                    State::Idle
                }
                // TODO: Don't drop buffer if in range..
                _ => {
                    let next_byte_offset = match pos {
                        SeekFrom::Start(start) => start,
                        SeekFrom::End(_end) => unimplemented!(),
                        SeekFrom::Current(pos) => (self.byte_offset as i64 + pos) as u64,
                    };
                    let seek_future = seek_future(self.core.clone(), next_byte_offset as usize);
                    byte_offset = next_byte_offset as usize;

                    State::Seeking(seek_future)
                }
            };
            self.byte_offset = byte_offset;
            self.seq = seq;
            self.block_offset = block_offset;
        }
        Poll::Ready(Ok(self.byte_offset as u64))
    }
}

impl AsyncRead for ByteStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        let mut finished = false;
        let mut pos = 0;
        while !finished {
            let mut block_offset = self.block_offset;
            let mut seq = self.seq;
            self.state = match &mut self.state {
                // TODO: Check if there is a situation where this would happen.
                State::Seeking(_) => unreachable!(),
                State::Finished => return Poll::Ready(Ok(0)),
                State::Idle => State::Reading(get_future(self.core.clone(), self.seq)),
                State::Reading(ref mut fut) => {
                    let poll_result = Pin::as_mut(fut).poll(cx);
                    let (_, block) = futures::ready!(poll_result);
                    match block {
                        // TODO: Treat only "Block not available from peers" as Finished, otherwise
                        // as error.
                        Err(_err) => State::Finished,
                        Ok(block) => State::Ready {
                            block: block.unwrap(),
                        },
                    }
                }
                State::Ready { block } => {
                    let block_slice = &block[block_offset..];
                    let buf_slice = &buf[pos..];
                    let len = std::cmp::min(buf_slice.len(), block_slice.len());
                    buf[pos..pos + len].copy_from_slice(&block_slice[..len]);
                    pos += len;
                    block_offset += len;
                    // We've read something, so return that.
                    finished = true;
                    if block_offset == block.len() {
                        block_offset = 0;
                        seq += 1;
                        State::Idle
                    } else {
                        // TODO: Don't clone....
                        State::Ready {
                            block: block.clone(),
                        }
                    }
                }
            };
            self.seq = seq;
            self.block_offset = block_offset;
        }
        Poll::Ready(Ok(pos))
    }
}

// async fn read_from(mut core: RemoteHypercore, start: FeedPos, buf: &mut [u8]) -> Result<usize> {
//     let (mut seq, mut offset) = match start {
//         FeedPos::Bytes(byte_offset) => core.seek(byte_offset as u64).await?,
//         FeedPos::Block(seq, offset) => (seq, offset),
//     };
//     let mut pos = 0;

//     let (n, seq, offset) = loop {
//         let slice = &mut buf[pos..];

//         let block = core.get(seq).await?.unwrap();
//         let block_slice = &block[offset..];
//         let len = std::cmp::min(block_slice.len(), slice.len());
//         slice[..len].copy_from_slice(&block_slice[..len]);
//         pos += len;
//         offset += len;
//         if offset >= block.len() {
//             offset = 0;
//             seq += 1;
//         }
//         if pos == buf.len() {
//             break (pos, seq, offset);
//         }
//     };
//     Ok(n)
// }
// enum FeedPos {
//     Bytes(usize),
//     Block(u64, usize),
// }
