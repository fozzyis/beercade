cmake_minimum_required(VERSION 3.10)
project(astro_rt CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED True)

# Create the static library for astro_rt
add_library(astro_rt STATIC src/input_evdev.cpp)

# Make the include directory public so consumers automatically get the path
target_include_directories(astro_rt PUBLIC include)

# Link against pthread for POSIX threading
target_link_libraries(astro_rt PUBLIC pthread)

# Ensure strict compilation
target_compile_options(astro_rt PRIVATE -Wall -Wextra -Werror -O3)
