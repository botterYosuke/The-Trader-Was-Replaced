"""BackendEventBus — threadsafe な fan-out バス（Phase 9 §3.12 / Step 0）。

責務:
- publish(event): 現在 subscribe 中の全 subscriber に event を配る
- subscribe(): blocking iterator (_Subscription) を返す（以降の publish を受信、過去は replay しない）
- close(): 全 subscriber に終端 sentinel を流し、以降の publish を RuntimeError で拒否

threading モデル:
- gRPC servicer は sync ThreadPool（handler は def）。SubscribeBackendEvents
  ストリーミング handler は worker thread でブロックし、publish は別 thread から
  呼ばれる。したがって asyncio ではなく queue.Queue + threading.Lock で実装する
  （market-data 用 LiveEventBus とは別物）。

payload に対して generic（proto import を持たない）。topic / filtering は持たない。
"""
from __future__ import annotations

import queue
import threading
from typing import Generic, Iterator, TypeVar

T = TypeVar("T")

_SENTINEL = object()


class _Subscription(Generic[T]):
    """1 subscriber 分の blocking iterator。queue から取り出し、sentinel で終端する。"""

    def __init__(self, bus: "BackendEventBus[T]") -> None:
        self._bus = bus
        self._queue: "queue.Queue[object]" = queue.Queue()
        self._closed = False

    def _put(self, item: object) -> None:
        self._queue.put(item)

    def __iter__(self) -> Iterator[T]:
        return self

    def __next__(self) -> T:
        item = self._queue.get()
        if item is _SENTINEL:
            raise StopIteration
        return item  # type: ignore[return-value]

    def close(self) -> None:
        """subscriber を bus から外し、ブロック中の __next__ を解放する。"""
        if self._closed:
            return
        self._closed = True
        self._bus._remove(self)
        self._queue.put(_SENTINEL)


class BackendEventBus(Generic[T]):
    def __init__(self) -> None:
        self._subscribers: list[_Subscription[T]] = []
        self._closed = False
        self._lock = threading.Lock()

    def subscribe(self) -> _Subscription[T]:
        sub: _Subscription[T] = _Subscription(self)
        with self._lock:
            if self._closed:
                # close 済みなら即終端する subscription を返す
                sub._put(_SENTINEL)
            else:
                self._subscribers.append(sub)
        return sub

    def publish(self, event: T) -> None:
        with self._lock:
            if self._closed:
                raise RuntimeError("BackendEventBus is closed")
            subs = list(self._subscribers)
        for sub in subs:
            sub._put(event)

    def _remove(self, sub: _Subscription[T]) -> None:
        with self._lock:
            if sub in self._subscribers:
                self._subscribers.remove(sub)

    def close(self) -> None:
        with self._lock:
            if self._closed:
                return
            self._closed = True
            subs = list(self._subscribers)
            self._subscribers.clear()
        for sub in subs:
            sub._put(_SENTINEL)
