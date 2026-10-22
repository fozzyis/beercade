#!/usr/bin/env bash
# ==============================================================================
# O.R.I.O.N. Active PWM Thermal Curve Controller (RK3588 / Radxa Rock 5C)
# Dynamically adjusts PWM fan curve based on SoC thermal zone 0 & 1 temperatures.
# ==============================================================================
set -euo pipefail

PWM_CHIP="${PWM_CHIP:-/sys/class/pwm/pwmchip1}"
PWM_CHANNEL="${PWM_CHANNEL:-pwm0}"
PERIOD_NS=25000 # 40kHz PWM

# Ensure PWM channel is exported
if [ ! -d "${PWM_CHIP}/${PWM_CHANNEL}" ]; then
    echo 0 > "${PWM_CHIP}/export" 2>/dev/null || true
fi

if [ -f "${PWM_CHIP}/${PWM_CHANNEL}/period" ]; then
    echo "${PERIOD_NS}" > "${PWM_CHIP}/${PWM_CHANNEL}/period" 2>/dev/null || true
    echo 1 > "${PWM_CHIP}/${PWM_CHANNEL}/enable" 2>/dev/null || true
fi

get_max_temp() {
    local t0 t1 max_t
    t0=$(cat /sys/class/thermal/thermal_zone0/temp 2>/dev/null || echo 40000)
    t1=$(cat /sys/class/thermal/thermal_zone1/temp 2>/dev/null || echo 40000)
    max_t=$(( t0 > t1 ? t0 : t1 ))
    echo $(( max_t / 1000 ))
}

set_fan_duty() {
    local duty_pct=$1
    local duty_ns=$(( (PERIOD_NS * duty_pct) / 100 ))
    if [ -f "${PWM_CHIP}/${PWM_CHANNEL}/duty_cycle" ]; then
        echo "${duty_ns}" > "${PWM_CHIP}/${PWM_CHANNEL}/duty_cycle" 2>/dev/null || true
    fi
}

echo "[+] Starting O.R.I.O.N. PWM fan daemon..."
while true; do
    TEMP=$(get_max_temp)
    if [ "$TEMP" -ge 75 ]; then
        set_fan_duty 100
    elif [ "$TEMP" -ge 65 ]; then
        set_fan_duty 70
    elif [ "$TEMP" -ge 55 ]; then
        set_fan_duty 45
    elif [ "$TEMP" -ge 45 ]; then
        set_fan_duty 25
    else
        set_fan_duty 0
    fi
    sleep 3
done
