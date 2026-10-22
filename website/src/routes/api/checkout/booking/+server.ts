import { json, type RequestHandler } from '@sveltejs/kit';

export const POST: RequestHandler = async ({ request }) => {
  try {
    const data = (await request.json()) as Record<string, any>;
    const {
      eventType,
      date,
      time,
      headcount,
      area,
      foodPreOrders = [],
      drinkBundle,
      tokenAddonAmount = 0,
      contactName,
      contactEmail,
      contactPhone
    } = data;

    if (!eventType || !date || !headcount || !area || !contactEmail) {
      return json({ error: 'Missing required booking parameters' }, { status: 400 });
    }

    // Deposit calculation ($20 per headcount minimum deposit, or flat rate)
    const depositPerPerson = 20;
    const baseDeposit = Number(headcount) * depositPerPerson;

    // Calculate food cost
    const foodTotal = foodPreOrders.reduce((sum: number, item: { price: number; quantity: number }) => {
      return sum + (item.price * item.quantity);
    }, 0);

    const totalAmount = baseDeposit + foodTotal + Number(tokenAddonAmount);

    const bookingRef = `BK-RES-${Date.now().toString(36).toUpperCase()}`;

    return json({
      success: true,
      bookingRef,
      depositAmount: baseDeposit,
      foodTotal,
      tokenAddonAmount,
      totalPaid: totalAmount,
      status: 'CONFIRMED',
      summary: {
        eventType,
        date,
        time,
        headcount,
        area,
        contactName,
        contactEmail,
        contactPhone
      },
      calendarEventUrl: `https://api.beercade.com.au/calendar/event/${bookingRef}.ics`,
      message: 'Function deposit and booking locked successfully!'
    });
  } catch (err) {
    return json({ error: 'Failed to process function booking' }, { status: 500 });
  }
};
