import { json, type RequestHandler } from '@sveltejs/kit';

export const GET: RequestHandler = async () => {
  const telemetryData = {
    timestamp: new Date().toISOString(),
    venue_status: 'OPEN',
    machines_total: 36,
    machines_active: 28,
    astro_nodes: [
      {
        node_id: 'ASTRO-01',
        hardware: 'ASTRO Arcade Hardware',
        current_core: 'fbneo',
        current_game: 'Street Fighter II: Champion Edition',
        status: 'IN_USE',
        session_duration_sec: 412,
        stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
      },
      {
        node_id: 'ASTRO-02',
        hardware: 'ASTRO Arcade Hardware',
        current_core: 'pcsx2',
        current_game: 'Tekken 3',
        status: 'AVAILABLE',
        session_duration_sec: 0,
        stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
      },
      {
        node_id: 'ASTRO-03',
        hardware: 'ASTRO Arcade Hardware',
        current_core: 'mame2003_plus',
        current_game: 'Metal Slug 3',
        status: 'IN_USE',
        session_duration_sec: 1250,
        stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
      },
      {
        node_id: 'ASTRO-04',
        hardware: 'ASTRO Arcade Hardware',
        current_core: 'flycast',
        current_game: 'Marvel vs. Capcom 2',
        status: 'IN_USE',
        session_duration_sec: 680,
        stream_url: 'https://cdn.sanity.io/files/pd409r0v/production/7d6f6089f0a96ecf68fcfd4510cdb3dc6d87b4ca.mp4'
      }
    ],
    featured_highscores: [
      { game: 'Donkey Kong', score: 114200, initials: 'AND' },
      { game: 'Attack from Mars', score: 489201940, initials: 'VAL' },
      { game: 'Street Fighter II', score: 980500, initials: 'RYU' },
      { game: 'Galaga', score: 254890, initials: 'ACE' }
    ]
  };

  return json(telemetryData);
};
