#include "astro_rt/input_events.h"

#include <cassert>
#include <climits>
#include <cstdint>
#include <cstdio>

namespace {

void test_degenerate_ranges() {
    assert(astro_input_scale_abs(0, 0, 0) == 0);
    assert(astro_input_scale_abs(50, 100, 100) == 0);
    assert(astro_input_scale_abs(50, 200, 100) == 0);
    assert(astro_input_scale_abs(0, INT32_MAX, INT32_MIN) == 0);
}

void test_eight_bit_encoder() {
    assert(astro_input_scale_abs(0, 0, 255) == -32768);
    assert(astro_input_scale_abs(255, 0, 255) == 32767);
    assert(astro_input_scale_abs(-50, 0, 255) == -32768);
    assert(astro_input_scale_abs(500, 0, 255) == 32767);

    int16_t prev = astro_input_scale_abs(0, 0, 255);
    for (int32_t v = 1; v <= 255; ++v) {
        const int16_t cur = astro_input_scale_abs(v, 0, 255);
        assert(cur > prev);
        prev = cur;
    }
}

void test_symmetric_and_identity_ranges() {
    for (int32_t v = -32768; v <= 32767; v += 127) {
        assert(astro_input_scale_abs(v, -32768, 32767) == static_cast<int16_t>(v));
    }
    assert(astro_input_scale_abs(32767, -32768, 32767) == 32767);
    assert(astro_input_scale_abs(-50000, -32768, 32767) == -32768);
    assert(astro_input_scale_abs(50000, -32768, 32767) == 32767);

    assert(astro_input_scale_abs(-32767, -32767, 32767) == -32768);
    assert(astro_input_scale_abs(0, -32767, 32767) == 0);
    assert(astro_input_scale_abs(32767, -32767, 32767) == 32767);

    assert(astro_input_scale_abs(1000, 1000, 2000) == -32768);
    assert(astro_input_scale_abs(1500, 1000, 2000) == 0);
    assert(astro_input_scale_abs(2000, 1000, 2000) == 32767);
}

void test_extreme_int32_span() {
    assert(astro_input_scale_abs(INT32_MIN, INT32_MIN, INT32_MAX) == -32768);
    assert(astro_input_scale_abs(0, INT32_MIN, INT32_MAX) == 0);
    assert(astro_input_scale_abs(INT32_MAX, INT32_MIN, INT32_MAX) == 32767);
}

} // namespace

int main() {
    test_degenerate_ranges();
    test_eight_bit_encoder();
    test_symmetric_and_identity_ranges();
    test_extreme_int32_span();
    std::printf("astro_rt_abs_scale_test: all checks passed\n");
    return 0;
}
