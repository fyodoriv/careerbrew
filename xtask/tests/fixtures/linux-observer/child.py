import errno
import os
import socket

path = "/work/transient"
marker = b"CAREERBREW-TRANSIENT-V1\n"
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
assert os.write(fd, marker) == len(marker)
os.close(fd)
os.unlink(path)
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as connection:
    try:
        connection.connect(("127.0.0.1", 9))
    except OSError as error:
        assert error.errno == errno.ECONNREFUSED
    else:
        raise AssertionError("unexpected loopback service")
