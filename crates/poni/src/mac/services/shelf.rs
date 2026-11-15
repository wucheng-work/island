//! 用户拖入文件时进行一次内容检查；结束后无常驻任务。

use crate::events::{AppEvent, EventSink};
use dispatch2::{DispatchQueue, DispatchQueueGlobalPriority, GlobalQueueIdentifier};
use poni_core::shelf::Shelf;

pub fn prepare(shelf: Shelf, paths: Vec<String>, sink: EventSink) {
    DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(
        DispatchQueueGlobalPriority::Low,
    ))
    .exec_async(move || {
        let result = shelf.prepare_paths(paths.clone());
        sink.send(AppEvent::ShelfPrepared {
            before: shelf.items,
            requested: paths,
            result,
        });
    });
}
