# Memory usage benchmark
## Based on the official implementation : https://socket.io/docs/v4/memory-usage/

The goal of this program is to benchmark the memory usage of the socket.io server under different conditions.
The server can be configured to generated its own reports with the `custom-report` feature flag.

The best way is still to run the program with (heaptrack)[https://github.com/KDE/heaptrack] and analyze the results with the `heaptrack_gui` tool.