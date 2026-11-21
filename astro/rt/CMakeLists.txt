cmake_minimum_required(VERSION 3.10)
project(astro_rt CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED True)

# Create the static library for astro_rt
add_library(astro_rt STATIC src/input_evdev.cpp)

# Make the include directory public so consumers automatically get the path
target_include_directories(astro_rt PUBLIC include)

# Link against pthread for POSIX threading and rt for POSIX shm on Linux
target_link_libraries(astro_rt PUBLIC pthread $<$<PLATFORM_ID:Linux>:rt>)

# Ensure strict compilation
target_compile_options(astro_rt PRIVATE -Wall -Wextra -Werror -O3)

include(CTest)
if(BUILD_TESTING)
    add_executable(astro_rt_spsc_test tests/spsc_ring_test.cpp)
    target_include_directories(astro_rt_spsc_test PRIVATE include)
    target_link_libraries(astro_rt_spsc_test PRIVATE pthread)
    target_compile_options(astro_rt_spsc_test PRIVATE -Wall -Wextra -Werror -O3)
    add_test(NAME astro_rt_spsc_test COMMAND astro_rt_spsc_test)

    add_executable(astro_rt_abs_scale_test tests/abs_scale_test.cpp)
    target_link_libraries(astro_rt_abs_scale_test PRIVATE astro_rt)
    target_compile_options(astro_rt_abs_scale_test PRIVATE -Wall -Wextra -Werror -O3)
    add_test(NAME astro_rt_abs_scale_test COMMAND astro_rt_abs_scale_test)

    add_executable(astro_rt_hist_shm_test tests/hist_shm_test.cpp)
    target_link_libraries(astro_rt_hist_shm_test PRIVATE astro_rt)
    target_compile_options(astro_rt_hist_shm_test PRIVATE -Wall -Wextra -Werror -O3)
    add_test(NAME astro_rt_hist_shm_test COMMAND astro_rt_hist_shm_test)

    add_executable(astro_rt_menu_bridge_test tests/menu_bridge_test.cpp)
    target_link_libraries(astro_rt_menu_bridge_test PRIVATE astro_rt)
    target_compile_options(astro_rt_menu_bridge_test PRIVATE -Wall -Wextra -Werror -O3)
    add_test(NAME astro_rt_menu_bridge_test COMMAND astro_rt_menu_bridge_test)
endif()
