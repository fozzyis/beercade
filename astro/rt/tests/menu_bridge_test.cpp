#include "astro_rt/input_events.h"

#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace {

uint8_t recv_menu_byte(int sock_fd) {
    struct pollfd pfd{};
    pfd.fd = sock_fd;
    pfd.events = POLLIN;
    const int prc = poll(&pfd, 1, 1000);
    assert(prc == 1);
    uint8_t code = 0xFF;
    const ssize_t n = recv(sock_fd, &code, 1, 0);
    assert(n == 1);
    return code;
}

void assert_no_menu_byte(int sock_fd) {
    struct pollfd pfd{};
    pfd.fd = sock_fd;
    pfd.events = POLLIN;
    const int prc = poll(&pfd, 1, 20);
    assert(prc == 0);
}

void test_menu_datagrams_and_grab_suppression() {
    const int pid = static_cast<int>(getpid());
    char sock_path[128]{};
    std::snprintf(sock_path, sizeof(sock_path), "/tmp/astro_rt_menu_test_%d.sock", pid);
    unlink(sock_path);

    char shm_name[128]{};
    std::snprintf(shm_name, sizeof(shm_name), "/astro_rt_menu_test_%d", pid);
    shm_unlink(shm_name);

    const int srv = socket(AF_UNIX, SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
    assert(srv >= 0);

    struct sockaddr_un addr{};
    addr.sun_family = AF_UNIX;
    assert(std::strlen(sock_path) < sizeof(addr.sun_path));
    std::memcpy(addr.sun_path, sock_path, std::strlen(sock_path) + 1);
    assert(bind(srv, reinterpret_cast<const struct sockaddr*>(&addr), sizeof(addr)) == 0);

    const int shm_fd = shm_open(shm_name, O_CREAT | O_RDWR | O_CLOEXEC, 0600);
    assert(shm_fd >= 0);
    assert(ftruncate(shm_fd, sizeof(uint32_t)) == 0);
    void* shm_ptr = mmap(nullptr, sizeof(uint32_t), PROT_READ | PROT_WRITE, MAP_SHARED, shm_fd, 0);
    assert(shm_ptr != MAP_FAILED);
    auto* grab = static_cast<uint32_t*>(shm_ptr);
    __atomic_store_n(grab, 0u, __ATOMIC_RELEASE);

    assert(setenv("ASTRO_RT_MENU_SOCK", sock_path, 1) == 0);
    assert(setenv("ASTRO_RT_MENU_SHM", shm_name, 1) == 0);

    astro_input_config cfg{};
    cfg.fifo_priority = 0;
    cfg.pin_cpu_first = 0;
    cfg.pin_cpu_last = 0;
    cfg.mlockall_enable = 0;
    cfg.ppoll_timeout_ms = 0;
    cfg.log_level = ASTRO_LOG_SILENT;
    assert(astro_input_start(&cfg) == 0);

    // 1. With grab == 0, game events reach the SPSC ring and menu hotkeys emit datagrams.
    assert(astro_input_inject_test_event(EV_KEY, KEY_TAB, 1, 100) == 1);
    assert(recv_menu_byte(srv) == ASTRO_MENU_TOGGLE);
    assert(astro_input_inject_test_event(EV_KEY, KEY_UP, 1, 200) == 1);
    assert(recv_menu_byte(srv) == ASTRO_MENU_UP);
    assert(astro_input_inject_test_event(EV_KEY, KEY_ENTER, 1, 300) == 1);
    assert(recv_menu_byte(srv) == ASTRO_MENU_OK);

    struct astro_input_event drained[16]{};
    assert(astro_input_drain(drained, 16) == 3);

    // 2. With grab == 1, menu datagrams still emit, but RetroArch drain() sees zero events.
    __atomic_store_n(grab, 1u, __ATOMIC_RELEASE);
    assert(astro_input_inject_test_event(EV_KEY, BTN_SOUTH, 1, 400) == 0);
    assert(recv_menu_byte(srv) == ASTRO_MENU_OK);
    assert(astro_input_inject_test_event(EV_KEY, KEY_DOWN, 1, 500) == 0);
    assert(recv_menu_byte(srv) == ASTRO_MENU_DOWN);
    assert(astro_input_inject_test_event(EV_KEY, KEY_ESC, 1, 600) == 0);
    assert(recv_menu_byte(srv) == ASTRO_MENU_BACK);
    assert(astro_input_inject_test_event(EV_ABS, ABS_HAT0Y, -1, 700) == 0);
    assert(recv_menu_byte(srv) == ASTRO_MENU_UP);
    assert(astro_input_inject_test_event(EV_ABS, ABS_HAT0Y, 0, 750) == 0);
    assert_no_menu_byte(srv);

    // START+SELECT combo fires Toggle once until released.
    assert(astro_input_inject_test_event(EV_KEY, BTN_START, 1, 800) == 0);
    assert_no_menu_byte(srv);
    assert(astro_input_inject_test_event(EV_KEY, BTN_SELECT, 1, 810) == 0);
    assert(recv_menu_byte(srv) == ASTRO_MENU_TOGGLE);
    assert(astro_input_inject_test_event(EV_KEY, BTN_SELECT, 0, 820) == 0);
    assert(astro_input_inject_test_event(EV_KEY, BTN_SELECT, 1, 830) == 0);
    assert_no_menu_byte(srv);

    assert(astro_input_drain(drained, 16) == 0);

    // 3. Releasing grab restores game forwarding.
    __atomic_store_n(grab, 0u, __ATOMIC_RELEASE);
    assert(astro_input_inject_test_event(EV_KEY, BTN_WEST, 1, 900) == 1);
    assert(astro_input_drain(drained, 16) == 1);
    assert(drained[0].code == BTN_WEST);

    astro_input_stop();

    assert(munmap(shm_ptr, sizeof(uint32_t)) == 0);
    assert(close(shm_fd) == 0);
    assert(close(srv) == 0);
    assert(shm_unlink(shm_name) == 0);
    assert(shm_unlink(ASTRO_RT_STATS_SHM_NAME) == 0);
    assert(unlink(sock_path) == 0);
    unsetenv("ASTRO_RT_MENU_SOCK");
    unsetenv("ASTRO_RT_MENU_SHM");
}

} // namespace

int main() {
    test_menu_datagrams_and_grab_suppression();
    std::printf("astro_rt_menu_bridge_test: all checks passed\n");
    return 0;
}
