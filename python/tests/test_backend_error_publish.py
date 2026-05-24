import asyncio
import queue

from engine.core import DataEngine
from engine.live.account_sync import AccountSync
from engine.live.adapter import VenueCredentials
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.state_machine import VenueStateMachine
from engine.mode_manager import ModeManager
from engine.proto import engine_pb2
from engine.server_grpc import GrpcDataEngineServer


def _make_servicer():
    venue_sm = VenueStateMachine()
    engine = DataEngine(state_machine=venue_sm)
    mm = ModeManager(venue_sm, engine)
    engine.attach_mode_manager(mm)
    return GrpcDataEngineServer(
        "test-token", engine, mode_manager=mm, venue_sm=venue_sm
    )


class _FailingComponent:
    async def start(self):
        raise RuntimeError("boom-start")


class _TimeoutStartComponent:
    async def start(self):
        raise asyncio.TimeoutError()


def test_bg_component_start_failure_publishes_backend_error():
    """`_start_bg_component_after_login` の start 失敗を握り潰さず
    BackendError として backend event stream に publish する（issue #29 D2 / B 経路）。"""
    servicer = _make_servicer()
    sub = servicer._backend_event_bus.subscribe()

    servicer._start_bg_component_after_login(_FailingComponent(), "account sync")

    # bounded 取得: publish されなければ queue.Empty で fast fail（ハングさせない）。
    # GREEN 後は publish された 1 件をすぐ取れる。
    received = sub._queue.get(timeout=2.0)
    assert received.WhichOneof("payload") == "backend_error"
    assert received.backend_error.source == "server_grpc"
    assert "boom-start" in received.backend_error.detail
    assert "account sync" in received.backend_error.detail


def test_account_sync_fetch_failure_publishes_backend_error():
    """AccountSync tick の fetch_account 失敗を server_grpc が拾い、
    BackendError(source="account_sync") として backend event stream に publish する
    （issue #29 D2 / A 経路）。"""
    servicer = _make_servicer()
    sub = servicer._backend_event_bus.subscribe()

    class _AlwaysFailAdapter(MockVenueAdapter):
        async def fetch_account(self):  # type: ignore[override]
            raise RuntimeError("account fetch boom")

    async def scenario():
        adapter = _AlwaysFailAdapter()
        await adapter.login(
            VenueCredentials(credentials_source="env", environment_hint="demo")
        )
        sync = AccountSync(
            adapter,
            on_account_event=servicer._publish_account_snapshot,
            on_error=servicer._publish_account_sync_error,
            interval_s=0.01,
        )
        await sync.start()
        for _ in range(40):
            if sync.last_error_record is not None:
                break
            await asyncio.sleep(0.005)
        await sync.stop()

    asyncio.run(scenario())

    received = sub._queue.get(timeout=2.0)
    assert received.WhichOneof("payload") == "backend_error"
    assert received.backend_error.source == "account_sync"
    assert "account fetch boom" in received.backend_error.detail


def test_account_sync_empty_message_exception_publishes_typed_detail():
    """`str(exc)` が空の例外（asyncio.TimeoutError 等）でも detail が空にならず
    型名を含むことを確認する（issue #29 Slice1 ②-A / 空 body 診断トースト回避）。"""
    servicer = _make_servicer()
    sub = servicer._backend_event_bus.subscribe()

    class _TimeoutAdapter(MockVenueAdapter):
        async def fetch_account(self):  # type: ignore[override]
            raise asyncio.TimeoutError()

    async def scenario():
        adapter = _TimeoutAdapter()
        await adapter.login(
            VenueCredentials(credentials_source="env", environment_hint="demo")
        )
        sync = AccountSync(
            adapter,
            on_account_event=servicer._publish_account_snapshot,
            on_error=servicer._publish_account_sync_error,
            interval_s=0.01,
        )
        await sync.start()
        for _ in range(40):
            if sync.last_error_record is not None:
                break
            await asyncio.sleep(0.005)
        await sync.stop()

    asyncio.run(scenario())

    received = sub._queue.get(timeout=2.0)
    assert received.WhichOneof("payload") == "backend_error"
    assert received.backend_error.source == "account_sync"
    assert received.backend_error.detail != ""
    assert "TimeoutError" in received.backend_error.detail


def test_bg_component_start_failure_empty_message_publishes_typed_detail():
    """`str(exc)` が空の start 例外（asyncio.TimeoutError 等）でも detail が空にならず
    型名を含むことを確認する（issue #29 Slice1 ②-B / 空 body 診断トースト回避・B 経路）。"""
    servicer = _make_servicer()
    sub = servicer._backend_event_bus.subscribe()

    servicer._start_bg_component_after_login(_TimeoutStartComponent(), "account sync")

    received = sub._queue.get(timeout=2.0)
    assert received.backend_error.source == "server_grpc"
    assert "account sync" in received.backend_error.detail   # label は残る
    assert "TimeoutError" in received.backend_error.detail    # 型名が残る（②-B の主眼）


def test_bg_component_start_failure_does_not_raise_when_bus_closed():
    """bus が closed のとき `_start_bg_component_after_login` の start 失敗は
    best-effort で握り潰し、publish 例外を呼び出し元へ伝播させない
    （issue #29 Slice1 レビュー指摘 #1 / B 経路の非対称性修正）。"""
    servicer = _make_servicer()
    # shutdown 中相当: bus を closed にすると publish() は RuntimeError を raise する。
    servicer._backend_event_bus.close()

    # start 失敗 → except に入り publish を試みる。現状 publish は unguarded なので
    # RuntimeError("BackendEventBus is closed") が伝播し、このテストは raise で fail（RED）。
    # GREEN 後は publish 失敗が握り潰され、何も raise せず返る。
    servicer._start_bg_component_after_login(_FailingComponent(), "account sync")
