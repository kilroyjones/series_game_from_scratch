import websocket
import time
import threading

def on_message(ws, message):
    print("Received from server: " + message)

def on_error(ws, error):
    print("Error: " + str(error))

def on_close(ws, close_status_code, close_msg):
    print("### Closed ###")
    print("Close status code: ", close_status_code)
    print("Close message: ", close_msg)

def on_open(ws):
    def run(*args):
        for i in range(10):
            time.sleep(3)
            message = "Hello Server {}".format(i)
            ws.send(message)
            print("Sent to server: " + message)
        time.sleep(1)
        ws.close()
        print("Thread terminating...")
    thread = threading.Thread(target=run)
    thread.start()

if __name__ == "__main__":
    websocket.enableTrace(True)
    ws = websocket.WebSocketApp("ws://127.0.0.1:8080/",
                                on_open=on_open,
                                on_message=on_message,
                                on_error=on_error,
                                on_close=on_close)
    
    ws.run_forever()
