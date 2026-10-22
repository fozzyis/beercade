import { json } from '@sveltejs/kit';

export async function POST({ request }) {
    const data: any = await request.json();
    console.log('[+] Booking received:', data);
    
    // Scaffold: In reality, we'd save this to D1 and potentially push to BOSS
    // if the booking is an immediate game launch.
    
    // Example: Forward a launch game request to BOSS if immediate
    if (data.immediate_launch) {
        try {
            // In a real SvelteKit app, these would come from env or platform variables.
            const bossUrl = 'http://boss.lan:8080';
            const token = '';
            const nodeId = data.node_id || 'ORION-01';
            
            await fetch(`${bossUrl}/api/v1/cabinet/${nodeId}/launch_game`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'Authorization': `Bearer ${token}`
                },
                body: JSON.stringify({
                    core: data.core || 'fbneo',
                    rom: data.rom || 'sf2ce',
                    netplay: data.netplay || false
                })
            });
        } catch (err) {
            console.error('Failed to communicate with BOSS:', err);
        }
    }

    return json({ success: true, booking_id: 'bkg_' + Date.now() });
}
