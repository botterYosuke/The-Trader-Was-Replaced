import logging
import time
from .models import TradingState

class DataEngine:
    def __init__(self):
        logging.info("Initializing DataEngine core")
        self.is_running = False
        self._price = 120.5
        self._history = [118.0, 119.0, 121.0, 120.5]

    def start(self):
        logging.info("Starting DataEngine core")
        self.is_running = True

    def stop(self):
        logging.info("Stopping DataEngine core")
        self.is_running = False

    def get_current_state(self) -> TradingState:
        return TradingState(
            price=self._price,
            history=self._history,
            timestamp=time.time()
        )
