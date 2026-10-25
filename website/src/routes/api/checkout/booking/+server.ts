import { json, type RequestHandler } from '@sveltejs/kit';

function buildIcsCalendar(params: {
	bookingRef: string;
	eventType: string;
	date: string;
	time: string;
	headcount: number;
	area: string;
}): string {
	const cleanDate = params.date.replace(/-/g, '');
	const cleanTime = (params.time || '18:00').replace(/:/g, '').padEnd(6, '0');
	const dtStart = `${cleanDate}T${cleanTime}`;
	return [
		'BEGIN:VCALENDAR',
		'VERSION:2.0',
		'PRODID:-//Beercade Redfern//Function Booking//EN',
		'BEGIN:VEVENT',
		`UID:${params.bookingRef}@beercade.com.au`,
		`DTSTART:${dtStart}`,
		`SUMMARY:Beercade Redfern — ${params.eventType} (${params.headcount} Guests)`,
		`LOCATION:Beercade Redfern (The BK)\\, 113-115 Regent St\\, Redfern NSW 2016 (${params.area})`,
		`DESCRIPTION:Booking Reference: ${params.bookingRef} | Area: ${params.area} | Guests: ${params.headcount}`,
		'END:VEVENT',
		'END:VCALENDAR'
	].join('\r\n');
}

export const GET: RequestHandler = async ({ url, platform }) => {
	const ref = url.searchParams.get('ref')?.trim() || 'BK-RES-DEMO';
	const format = url.searchParams.get('format');

	const db = platform?.env?.DB;
	let row: Record<string, unknown> | null = null;
	if (db) {
		try {
			row = await db
				.prepare(`SELECT * FROM function_bookings WHERE booking_ref = ?1 LIMIT 1`)
				.bind(ref)
				.first<Record<string, unknown>>();
		} catch {
			// Ignore if table not yet migrated locally
		}
	}

	if (format === 'ics') {
		const ics = buildIcsCalendar({
			bookingRef: ref,
			eventType: String(row?.event_type ?? url.searchParams.get('type') ?? 'Function Booking'),
			date: String(row?.booking_date ?? url.searchParams.get('date') ?? '2026-11-15'),
			time: String(row?.booking_time ?? url.searchParams.get('time') ?? '18:00'),
			headcount: Number(row?.headcount ?? url.searchParams.get('guests') ?? 12),
			area: String(row?.area ?? url.searchParams.get('area') ?? 'Pinball Alley')
		});
		return new Response(ics, {
			headers: {
				'Content-Type': 'text/calendar; charset=utf-8',
				'Content-Disposition': `attachment; filename="${ref}.ics"`
			}
		});
	}

	if (row) {
		return json({ success: true, booking: row });
	}
	return json({ error: 'Booking not found' }, { status: 404 });
};

export const POST: RequestHandler = async ({ request, platform, url }) => {
	try {
		const data = (await request.json()) as Record<string, any>;
		const {
			eventType,
			date,
			time,
			headcount,
			area,
			foodPreOrders = [],
			drinkBundle = 'Classic Bar Tab',
			tokenAddonAmount = 0,
			contactName,
			contactEmail,
			contactPhone
		} = data;

		if (!eventType || !date || !headcount || !area || !contactEmail) {
			return json({ error: 'Missing required booking parameters' }, { status: 400 });
		}

		const guests = Math.max(4, Math.min(200, Number(headcount)));
		const depositPerPerson = 20;
		const baseDeposit = guests * depositPerPerson;

		const foodTotal = Array.isArray(foodPreOrders)
			? foodPreOrders.reduce((sum: number, item: { price: number; quantity: number }) => {
					return sum + Number(item.price || 0) * Number(item.quantity || 0);
				}, 0)
			: 0;

		const addonAmt = Math.max(0, Number(tokenAddonAmount) || 0);
		const totalAmount = baseDeposit + foodTotal + addonAmt;
		const now = Date.now();
		const bookingRef = `BK-RES-${now.toString(36).toUpperCase()}`;

		const envRecord = (platform?.env ?? {}) as Record<string, unknown>;
		const stripeSecretKey =
			(typeof envRecord.STRIPE_SECRET_KEY === 'string' ? envRecord.STRIPE_SECRET_KEY : '') ||
			process.env.STRIPE_SECRET_KEY ||
			'';

		let stripeSessionId: string | null = null;
		let checkoutUrl: string | null = null;

		if (stripeSecretKey.trim()) {
			const Stripe = (await import('stripe')).default;
			const stripe = new Stripe(stripeSecretKey.trim(), {
				apiVersion: '2023-10-16' as any
			});

			const session = await stripe.checkout.sessions.create({
				customer_email: String(contactEmail).trim(),
				line_items: [
					{
						price_data: {
							currency: 'aud',
							product_data: {
								name: `Beercade Function Deposit — ${eventType} (${bookingRef})`,
								description: `${guests} Guests @ ${area} on ${date} ${time || '18:00'}`
							},
							unit_amount: totalAmount * 100
						},
						quantity: 1
					}
				],
				metadata: {
					booking_ref: bookingRef,
					area: String(area),
					headcount: String(guests)
				},
				mode: 'payment',
				success_url: `${url.origin}/bookings?ref=${encodeURIComponent(bookingRef)}&session_id={CHECKOUT_SESSION_ID}`,
				cancel_url: `${url.origin}/bookings?cancelled=1`
			});

			stripeSessionId = session.id;
			checkoutUrl = session.url;
		}

		const db = platform?.env?.DB;
		if (db) {
			try {
				await db
					.prepare(
						`INSERT INTO function_bookings
						 (booking_ref, event_type, booking_date, booking_time, headcount, area, drink_bundle,
						  food_preorders_json, deposit_amount, food_total, token_addon_amount, total_amount,
						  contact_name, contact_email, contact_phone, stripe_session_id, status, created_at_ms)
						 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)`
					)
					.bind(
						bookingRef,
						String(eventType),
						String(date),
						String(time || '18:00'),
						guests,
						String(area),
						String(drinkBundle),
						JSON.stringify(foodPreOrders),
						baseDeposit,
						foodTotal,
						addonAmt,
						totalAmount,
						String(contactName || 'Guest'),
						String(contactEmail),
						String(contactPhone || ''),
						stripeSessionId,
						checkoutUrl ? 'PENDING_STRIPE' : 'CONFIRMED',
						now
					)
					.run();
			} catch (dbErr) {
				console.warn('D1 function_bookings insert skipped:', dbErr);
			}
		}

		const calendarQuery = new URLSearchParams({
			ref: bookingRef,
			format: 'ics',
			type: String(eventType),
			date: String(date),
			time: String(time || '18:00'),
			guests: String(guests),
			area: String(area)
		});

		return json({
			success: true,
			bookingRef,
			depositAmount: baseDeposit,
			foodTotal,
			tokenAddonAmount: addonAmt,
			totalPaid: totalAmount,
			status: 'CONFIRMED',
			checkoutUrl,
			summary: {
				eventType,
				date,
				time: time || '18:00',
				headcount: guests,
				area,
				contactName,
				contactEmail,
				contactPhone
			},
			calendarEventUrl: `/api/checkout/booking?${calendarQuery.toString()}`,
			message: 'Function deposit and booking locked successfully!'
		});
	} catch (err) {
		console.error('Function booking failed:', err);
		return json({ error: 'Failed to process function booking' }, { status: 500 });
	}
};
