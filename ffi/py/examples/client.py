import argparse
import py_ws;
import logging
FORMAT = '%(levelname)s %(name)s %(asctime)-15s %(filename)s:%(lineno)d %(message)s'
logging.basicConfig(format=FORMAT)
logging.getLogger().setLevel(logging.DEBUG)

def main():
    parser = argparse.ArgumentParser(description="websocket client")
    parser.add_argument("url", help="websocket url")
    parser.add_argument("-c", "--cert", help="custom certification", action="append")
    parser.add_argument("-w", "--window", help="deflate window size", default=None)
    parser.add_argument("-b", "--buffer", help="deflate window size", default=None, type=int)

    args = parser.parse_args()
    config = py_ws.ClientConfig(window=args.window, certs=args.cert, buf=args.buf)
    client = py_ws.connect(args.url, config)
    client.text("Hello")
    client.flush()
    code, msg = client.recv()
    print(code, msg)
    client.text("World")
    client.flush()
    code, msg = client.recv()
    print(code, msg)
    client.close()


if __name__ == '__main__':
    main()