import argparse
import logging
import os
import sys
from .server_grpc import serve


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description="Headless Data Engine Backend")
    parser.add_argument("--port", type=int, default=19876, help="Port to listen on")
    parser.add_argument("--token", type=str, required=True, help="Authentication token")
    parser.add_argument(
        "--transport",
        type=str,
        default="grpc",
        choices=["grpc"],
        help="Protocol selection",
    )

    # Phase 5 Enhanced Backend Options
    parser.add_argument(
        "--max-history-len",
        type=int,
        default=1000,
        help="Maximum number of historical points to keep",
    )
    parser.add_argument(
        "--advance-interval-sec",
        type=float,
        default=1.0,
        help="Interval between data points in seconds",
    )

    # Phase 6 Nautilus Replay Integration
    parser.add_argument(
        "--jquants-dir", type=str, help="Path to J-Quants data directory"
    )
    parser.add_argument(
        "--jquants-catalog-path",
        type=str,
        default=os.environ.get("JQUANTS_CATALOG_PATH"),
        help="Path to Nautilus ParquetDataCatalog for J-Quants data (env: JQUANTS_CATALOG_PATH, ARTIFACTS_PATH)",
    )

    # Phase 8 §3.2 C1 Live venue wiring
    parser.add_argument(
        "--live-venue",
        type=str,
        default=None,
        choices=["TACHIBANA", "KABU"],
        help="Live venue adapter to wire (TACHIBANA or KABU). Default: None (Replay only).",
    )

    return parser.parse_args(argv)


def main():
    args = parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        handlers=[logging.StreamHandler(sys.stdout)],
    )

    logging.info(
        f"Starting engine backend (headless) on port {args.port} with {args.transport} transport"
    )

    if args.transport == "grpc":
        serve(
            args.port,
            args.token,
            auto_start=False,
            max_history_len=args.max_history_len,
            advance_interval_sec=args.advance_interval_sec,
            jquants_dir=args.jquants_dir,
            jquants_catalog_path=args.jquants_catalog_path,
            live_venue=args.live_venue,
        )
    else:
        logging.error(f"Unsupported transport: {args.transport}")
        sys.exit(1)


if __name__ == "__main__":
    main()
