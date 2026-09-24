set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)

set(CMAKE_C_COMPILER "${CMAKE_CURRENT_LIST_DIR}/zigcc-shim-arm64.sh")

set(CMAKE_AR aarch64-unknown-linux-gnu-ar)
set(CMAKE_RANLIB aarch64-unknown-linux-gnu-ranlib)
