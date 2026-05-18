"""Tests for kabusapi_register.RegisterSet — 50-symbol LRU (Phase 8 §3.2 B3).

Q-K5 決定: 暗黙 evict は行わない。満杯時は KabuRegisterFullError (code 4002001)。
on_evict は evict_lru() を明示的に呼んだ時のみ発火する。
"""

from unittest.mock import MagicMock

import pytest


class TestRegisterSetBasics:
    def setup_method(self):
        # Deferred import: RegisterSet is not implemented yet (Phase 8 §3.2 B3).
        from engine.exchanges.kabusapi_register import MAX_SYMBOLS, RegisterSet
        self._RegisterSet = RegisterSet
        self._MAX_SYMBOLS = MAX_SYMBOLS

    def test_max_symbols_const_is_50(self):
        assert self._MAX_SYMBOLS == 50

    def test_class_max_matches_module_const(self):
        assert self._RegisterSet.MAX == self._MAX_SYMBOLS

    def test_default_ctor_has_len_zero(self):
        rs = self._RegisterSet()
        assert len(rs) == 0

    def test_register_one_symbol_len_one(self):
        rs = self._RegisterSet()
        rs.register("7203", 1)
        assert len(rs) == 1

    def test_register_same_key_twice_is_idempotent(self):
        rs = self._RegisterSet()
        rs.register("7203", 1)
        rs.register("7203", 1)
        assert len(rs) == 1

    def test_contains_tuple_key(self):
        rs = self._RegisterSet()
        rs.register("7203", 1)
        assert ("7203", 1) in rs
        assert ("7203", 2) not in rs
        assert ("9999", 1) not in rs


class TestRegisterSetCapacity:
    def setup_method(self):
        from engine.exchanges.kabusapi_register import RegisterSet
        self._RegisterSet = RegisterSet

    def test_register_50_symbols_ok(self):
        rs = self._RegisterSet()
        for i in range(50):
            rs.register(f"S{i:04d}", 1)
        assert len(rs) == 50

    def test_register_51st_raises_full_error(self):
        from engine.exchanges.kabusapi_auth import KabuRegisterFullError
        rs = self._RegisterSet()
        for i in range(50):
            rs.register(f"S{i:04d}", 1)
        with pytest.raises(KabuRegisterFullError) as exc:
            rs.register("S0050", 1)
        assert exc.value.code == 4002001

    def test_existing_key_register_does_not_raise_when_full(self):
        rs = self._RegisterSet()
        for i in range(50):
            rs.register(f"S{i:04d}", 1)
        # 既存 key の re-register は touch 扱い → 満杯でも raise しない
        rs.register("S0000", 1)
        assert len(rs) == 50

    def test_no_implicit_eviction_on_full(self):
        """51 件目で raise されても既存 50 件は維持される (Q-K5)。"""
        from engine.exchanges.kabusapi_auth import KabuRegisterFullError
        rs = self._RegisterSet()
        for i in range(50):
            rs.register(f"S{i:04d}", 1)
        with pytest.raises(KabuRegisterFullError):
            rs.register("NEW", 1)
        assert len(rs) == 50
        assert ("S0000", 1) in rs  # 最古が暗黙 evict されていない
        assert ("NEW", 1) not in rs

    def test_custom_max_symbols(self):
        from engine.exchanges.kabusapi_auth import KabuRegisterFullError
        rs = self._RegisterSet(max_symbols=2)
        rs.register("A", 1)
        rs.register("B", 1)
        with pytest.raises(KabuRegisterFullError):
            rs.register("C", 1)


class TestRegisterSetUnregister:
    def setup_method(self):
        from engine.exchanges.kabusapi_register import RegisterSet
        self._RegisterSet = RegisterSet

    def test_unregister_existing_returns_true(self):
        rs = self._RegisterSet()
        rs.register("7203", 1)
        assert rs.unregister("7203", 1) is True
        assert len(rs) == 0

    def test_unregister_missing_returns_false(self):
        rs = self._RegisterSet()
        assert rs.unregister("9999", 1) is False

    def test_unregister_all_clears(self):
        rs = self._RegisterSet()
        rs.register("7203", 1)
        rs.register("9984", 1)
        rs.unregister_all()
        assert len(rs) == 0


class TestRegisterSetTouchAndLRU:
    def setup_method(self):
        from engine.exchanges.kabusapi_register import RegisterSet
        self._RegisterSet = RegisterSet

    def test_touch_missing_is_noop(self):
        rs = self._RegisterSet()
        # raise しない
        rs.touch("9999", 1)
        assert len(rs) == 0

    def test_touch_default_exchange_is_1(self):
        rs = self._RegisterSet()
        rs.register("7203", 1)
        # exchange 省略でも raise しない (= default 1 で touch)
        rs.touch("7203")
        assert ("7203", 1) in rs

    def test_all_symbols_returns_lru_order(self):
        rs = self._RegisterSet()
        rs.register("A", 1)
        rs.register("B", 1)
        rs.register("C", 1)
        assert rs.all_symbols() == [("A", 1), ("B", 1), ("C", 1)]

    def test_touch_moves_key_to_newest(self):
        rs = self._RegisterSet()
        rs.register("A", 1)
        rs.register("B", 1)
        rs.register("C", 1)
        rs.touch("A", 1)
        assert rs.all_symbols() == [("B", 1), ("C", 1), ("A", 1)]

    def test_re_register_existing_moves_to_newest(self):
        rs = self._RegisterSet()
        rs.register("A", 1)
        rs.register("B", 1)
        rs.register("A", 1)  # touch via register
        assert rs.all_symbols() == [("B", 1), ("A", 1)]


class TestRegisterSetEvictLRU:
    def setup_method(self):
        from engine.exchanges.kabusapi_register import RegisterSet
        self._RegisterSet = RegisterSet

    def test_evict_lru_empty_returns_none(self):
        rs = self._RegisterSet()
        assert rs.evict_lru() is None

    def test_evict_lru_returns_oldest_key(self):
        rs = self._RegisterSet()
        rs.register("A", 1)
        rs.register("B", 1)
        rs.register("C", 1)
        assert rs.evict_lru() == ("A", 1)
        assert ("A", 1) not in rs
        assert len(rs) == 2

    def test_evict_lru_fires_on_evict_callback(self):
        cb = MagicMock()
        rs = self._RegisterSet(on_evict=cb)
        rs.register("A", 1)
        rs.register("B", 2)
        result = rs.evict_lru()
        assert result == ("A", 1)
        cb.assert_called_once_with("A", 1)

    def test_evict_lru_without_callback_does_not_raise(self):
        rs = self._RegisterSet(on_evict=None)
        rs.register("A", 1)
        assert rs.evict_lru() == ("A", 1)

    def test_evict_lru_callback_not_called_when_empty(self):
        cb = MagicMock()
        rs = self._RegisterSet(on_evict=cb)
        assert rs.evict_lru() is None
        cb.assert_not_called()

    def test_evict_lru_respects_touch_order(self):
        """touch で最新化された key は evict されない。"""
        cb = MagicMock()
        rs = self._RegisterSet(on_evict=cb)
        rs.register("A", 1)
        rs.register("B", 1)
        rs.register("C", 1)
        rs.touch("A", 1)  # A を最新へ → 最古は B
        assert rs.evict_lru() == ("B", 1)
        cb.assert_called_once_with("B", 1)
