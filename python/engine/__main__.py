import argparse
import logging
import sys
from .server_grpc import serve

def main():
    parser = argparse.ArgumentParser(description="Headless Data Engine Backend")
    parser.add_argument("--port", type=int, default=19876, help="Port to listen on")
    parser.add_argument("--token", type=str, required=True, help="Authentication token")
    parser.add_argument("--transport", type=str, default="grpc", choices=["grpc"], help="Protocol selection")
    
    args = parser.parse_args()
    
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        handlers=[logging.StreamHandler(sys.stdout)]
    )
    
    logging.info(f"Starting engine backend (headless) on port {args.port} with {args.transport} transport")
    
    if args.transport == "grpc":
        serve(args.port, args.token)
    else:
        logging.error(f"Unsupported transport: {args.transport}")
        sys.exit(1)

if __name__ == "__main__":
    main()
