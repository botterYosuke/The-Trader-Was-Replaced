from pathlib import Path


class JQuantsLoader:
    def __init__(self, base_dir: str):
        self.base_dir = Path(base_dir)

    def check_data_exists(
        self,
        instrument_ids: list[str],
        start_date: str,
        end_date: str,
    ) -> bool:
        return self.base_dir.exists()
