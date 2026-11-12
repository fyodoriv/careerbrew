FROM python:3.12.16-slim-bookworm@sha256:e697c81897302a4c4333bade000d98038fbb21eb064f8ee1fd42d02ebcd9d337
RUN apt-get update && apt-get install -y --no-install-recommends strace=6.1-0.1 && rm -rf /var/lib/apt/lists/*
RUN strace --version && strace --help && python --version && dpkg-query -W strace
RUN strace -e trace=%process,%file,%network,write,close,close_range,dup,dup2,dup3,fcntl --version
COPY parent.py child.py supervisor.py export.py /probe/
WORKDIR /work
