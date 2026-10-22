import type { RequestHandler } from '@sveltejs/kit';

export const GET: RequestHandler = async () => {
  let interval: ReturnType<typeof setInterval>;

  const stream = new ReadableStream({
    start(controller) {
      const sendEvent = () => {
        try {
          const games = [
            { name: 'Street Fighter II: Champion Edition', core: 'fbneo' },
            { name: 'Tekken 2', core: 'pcsx2' },
            { name: 'Metal Slug 3', core: 'mame2003_plus' },
            { name: 'Marvel vs. Capcom 2', core: 'flycast' },
            { name: 'Pac-Man', core: 'mame2003' },
            { name: 'Daytona USA', core: 'model2' },
            { name: 'Godzilla Pinball', core: 'stern_pinball' },
            { name: 'Windjammers', core: 'fbneo' }
          ];

          const activeCount = Math.floor(Math.random() * 8) + 26;
          const randomGame = games[Math.floor(Math.random() * games.length)];

          const payload = {
            timestamp: new Date().toISOString(),
            venue_status: 'OPEN',
            machines_total: 36,
            machines_active: activeCount,
            astro_nodes: [
              {
                node_id: 'ASTRO-01',
                hardware: 'ASTRO Arcade Hardware',
                current_core: randomGame.core,
                current_game: randomGame.name,
                status: 'IN_USE',
                session_duration_sec: Math.floor(Math.random() * 900) + 60,
                stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
              },
              {
                node_id: 'ASTRO-02',
                hardware: 'ASTRO Arcade Hardware',
                current_core: 'pcsx2',
                current_game: 'Tekken 3',
                status: Math.random() > 0.4 ? 'IN_USE' : 'AVAILABLE',
                session_duration_sec: Math.floor(Math.random() * 400),
                stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
              },
              {
                node_id: 'ASTRO-03',
                hardware: 'ASTRO Arcade Hardware',
                current_core: 'mame2003_plus',
                current_game: 'Metal Slug 3',
                status: 'IN_USE',
                session_duration_sec: Math.floor(Math.random() * 1500) + 100,
                stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
              },
              {
                node_id: 'ASTRO-04',
                hardware: 'ASTRO Arcade Hardware',
                current_core: 'flycast',
                current_game: 'Marvel vs. Capcom 2',
                status: 'IN_USE',
                session_duration_sec: Math.floor(Math.random() * 800) + 50,
                stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
              }
            ],
            featured_highscores: [
              { game: 'Donkey Kong', score: 114200 + Math.floor(Math.random() * 100), initials: 'AND' },
              { game: 'Attack from Mars', score: 489201940, initials: 'VAL' },
              { game: 'Street Fighter II', score: 980500, initials: 'RYU' }
            ]
          };

          controller.enqueue(`data: ${JSON.stringify(payload)}\n\n`);
        } catch (e) {
          clearInterval(interval);
        }
      };

      sendEvent();
      interval = setInterval(sendEvent, 3000);
    },
    cancel() {
      if (interval) clearInterval(interval);
    }
  });

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      'Connection': 'keep-alive'
    }
  });
};
