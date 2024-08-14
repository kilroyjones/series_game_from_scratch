import socket

def interactive_echo_client(host='localhost', port=8080):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.connect((host, port))
        print(f"Connected to {host}:{port}")
        print("Type your messages (press Ctrl+C to exit):")

        
        try:
            while True:
                message = input("> ")
                if not message:
                    continue
                
                s.sendall(message.encode())
                data = s.recv(1024)
                print(f"Received: {data.decode()}")
        
        except KeyboardInterrupt:
            print("\nDisconnecting from server...")
        
        except Exception as e:
            print(f"An error occurred: {e}")

if __name__ == "__main__":
    interactive_echo_client()