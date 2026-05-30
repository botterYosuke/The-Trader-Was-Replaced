import os
import threading
import logging

_lock = threading.Lock()
_shutting_down: bool = False  # Health.Check が読む (lock 保護)
_shutdown_thread: threading.Thread | None = None
_components: dict = {}  # server / engine / servicer を登録 (lock 下で読み書き)


def set_components(
    *,
    server,
    engine,
    servicer,
) -> None:
    """_backend_impl.serve() の server.start() 直前に 1 度だけ呼ぶ。
    live loop thread の join は servicer._teardown_live_components() が内部で処理する
    ため、本モジュールでは live thread を別管理しない。"""
    with _lock:
        _components["server"] = server
        _components["engine"] = engine
        _components["servicer"] = servicer  # _teardown_live_components はメソッド


def is_shutting_down() -> bool:
    with _lock:
        return _shutting_down


def start_shutdown(grace_seconds: int = 3) -> bool:
    """Shutdown RPC ハンドラ / signal handler の両方から呼ぶ。
    戻り値: True = この呼び出しで shutdown thread を起動した。
            False = 既に shutdown 進行中で何もしなかった。
    多重呼び出しは構造的に無視 (lock で in-flight 判定)。"""
    global _shutdown_thread
    with _lock:
        if _shutdown_thread is not None:
            return False
        _shutdown_thread = threading.Thread(
            target=_shutdown_thread_main,
            args=(grace_seconds,),
            daemon=True,
            name="process_lifecycle_shutdown",
        )
        _shutdown_thread.start()
        return True


def _shutdown_thread_main(grace_seconds: int) -> None:
    global _shutting_down
    try:
        with _lock:
            _shutting_down = True   # Health.Check を NOT_SERVING へ
            components = dict(_components)  # スナップショット (以降は lock 外で読む)

        # Shutdown RPC レスポンスを wire に乗せきる猶予 (C-6 step 2)
        import time as _time
        _time.sleep(0.25)

        try:
            engine = components.get("engine")
            if engine is not None:
                engine.stop()       # 取引ループ停止 (同期)
        except Exception:
            logging.exception("engine.stop() failed during shutdown")

        try:
            servicer = components.get("servicer")
            if servicer is not None:
                servicer._teardown_live_components()
        except Exception:
            logging.exception("_teardown_live_components() failed during shutdown")

        server = components.get("server")
        if server is not None:
            try:
                event = server.stop(grace_seconds)  # threading.Event を返す
                event.wait(timeout=max(grace_seconds, 0) + 0.5)
            except Exception:
                logging.exception("server.stop() failed during shutdown")
    finally:
        # どんな例外経路でも必ず exit。さもないと再 shutdown が永久に no-op になる。
        os._exit(0)
