import { json } from '@sveltejs/kit';

export async function POST({ request }) {
    const data: any = await request.json();
    console.log('[+] Token purchase/add request received:', data);
    
    try {
        const bossUrl = 'http://boss.lan:8080';
        const token = '';
        const nodeId = data.node_id || 'ORION-01';
        const credits = data.credits || 1;
        
        await fetch(`${bossUrl}/api/v1/credits/add`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'Authorization': `Bearer ${token}`
            },
            body: JSON.stringify({
                node_id: nodeId,
                credits: credits
            })
        });
    } catch (err) {
        console.error('Failed to add credits via BOSS:', err);
    }

    return json({ success: true, tokens_added: data.credits || 1 });
}
