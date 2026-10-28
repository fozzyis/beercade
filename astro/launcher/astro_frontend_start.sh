#!/bin/sh
# =============================================================================
# Beercade A.S.T.R.O. - customer screen
# -----------------------------------------------------------------------------
# Runs astro-shell on KMS: call to action, the player's game list and the
# time's-up screen, all driven by astro-daemon. Games and attract mode come
# from B.O.S.S. via the daemon, which stops this service while a game is on
# screen and starts it again when the game exits.
# =============================================================================

mkdir -p /data/beercade/cache /data/beercade/saves /data/beercade/states /data/beercade/screenshots

exec /bin/sh /usr/lib/beercade/astro_kms_launcher.sh /usr/bin/astro-shell
