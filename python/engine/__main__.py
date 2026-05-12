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
    
    # Reserved for future use (e-station compatibility)
    parser.add_argument("--instrument-id", type=str, help="Reserved: Instrument ID")
    parser.add_argument("--start", type=str, help="Reserved: Start time")
    parser.add_argument("--end", type=str, help="Reserved: End time")
    parser.add_argument("--granularity", type=str, help="Reserved: Data granularity")
    parser.add_argument("--data-dir", type=str, help="Reserved: Data directory")

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
        replay_provider = SimpleCSVProvider(args.replay_path)
    
    if args.transport == "grpc":
        serve(args.port, args.token, replay_provider=replay_provider)
    else:
        logging.error(f"Unsupported transport: {args.transport}")
        sys.exit(1)

if __name__ == "__main__":
    main()
