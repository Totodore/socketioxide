# socketioxide-parser-common 0.16.1
* fix: clone partial packets when keeping them to avoid holding a reference to the ws read buffer for too long. Otherwise it cause the
ws read buffer to grows indefinitely in a many binary packets scenario.

# socketioxide-parser-common 0.16.0
* feat(*breaking*): remote adapters
