import argparse
import logging
import sys
from .server_grpc import serve
from .replay import SimpleCSVProvider

def main():
    parser = argparse.ArgumentParser(description="Headless Data Engine Backend")
    parser.add_argument("--port", type=int, default=19876, help="Port to listen on")
    parser.add_argument("--token", type=str, required=True, help="Authentication token")
    parser.add_argument("--transport", type=str, default="grpc", choices=["grpc"], help="Protocol selection")
    
    # Phase 3 Replay Options
    parser.add_argument("--mode", type=str, default="static", choices=["static", "replay"], help="Execution mode")
    parser.add_argument("--replay-path", type=str, help="Path to simple CSV for replay")
    parser.add_argument("--auto-start", action="store_true", help="Start engine progression immediately")

    # Phase 5 Enhanced Backend Options
    parser.add_argument("--max-history-len", type=int, default=1000, help="Maximum number of historical points to keep")
    parser.add_argument("--advance-interval-sec", type=float, default=1.0, help="Interval between data points in seconds")

    args = parser.parse_args()
    
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        handlers=[logging.StreamHandler(sys.stdout)]
    )
    
    logging.info(f"Starting engine backend (headless) on port {args.port} with {args.transport} transport")
    logging.info(f"Mode: {args.mode}")

    replay_provider = None
    if args.mode == "replay":
        if not args.replay_path:
            logging.error("--replay-path is required when --mode is 'replay'")
            sys.exit(1)
        try:
            replay_provider = SimpleCSVProvider(args.replay_path)
        except Exception as e:
            logging.error(f"Failed to initialize ReplayProvider: {e}")
            sys.exit(1)
    
    if args.transport == "grpc":
        # Static モードは常に自動進行させる（既存の互換性のため）
        # Replay モードは --auto-start に従う
        auto_start = args.auto_start if args.mode == "replay" else True
        serve(
            args.port, 
            args.token, 
            replay_provider=replay_provider, 
            auto_start=auto_start,
            max_history_len=args.max_history_len,
            advance_interval_sec=args.advance_interval_sec
        )
    else:
        logging.error(f"Unsupported transport: {args.transport}")
        sys.exit(1)

if __name__ == "__main__":
    main()
