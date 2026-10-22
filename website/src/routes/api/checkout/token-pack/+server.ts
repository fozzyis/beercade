import { json, type RequestHandler } from '@sveltejs/kit';

export const POST: RequestHandler = async ({ request }) => {
  try {
    const body = (await request.json()) as { amount: number; email: string; quantity?: number };
    const { amount, email, quantity = 1 } = body;

    const allowedTiers = [5, 10, 20, 50, 100, 200, 300, 500];
    if (!allowedTiers.includes(Number(amount))) {
      return json({ error: 'Invalid token pack tier' }, { status: 400 });
    }

    if (!email || !email.includes('@')) {
      return json({ error: 'Valid email is required for voucher delivery' }, { status: 400 });
    }

    // Calculate tokens (e.g. $1 = 1 token, bonus tokens on higher tiers)
    let bonusPercentage = 0;
    if (amount >= 500) bonusPercentage = 30;
    else if (amount >= 300) bonusPercentage = 25;
    else if (amount >= 200) bonusPercentage = 20;
    else if (amount >= 100) bonusPercentage = 15;
    else if (amount >= 50) bonusPercentage = 10;

    const baseTokens = Number(amount) * quantity;
    const bonusTokens = Math.floor(baseTokens * (bonusPercentage / 100));
    const totalTokens = baseTokens + bonusTokens;

    const stripeSecretKey = process.env.STRIPE_SECRET_KEY || '';
    if (!stripeSecretKey) {
      return json({ error: 'Stripe secret key not configured' }, { status: 500 });
    }

    const Stripe = (await import('stripe')).default;
    const stripe = new Stripe(stripeSecretKey, {
      apiVersion: '2023-10-16' as any,
    });

    const session = await stripe.checkout.sessions.create({
      customer_email: email,
      line_items: [
        {
          price_data: {
            currency: 'aud',
            product_data: {
              name: `${totalTokens} Tokens`,
              description: `Base: ${baseTokens} tokens + Bonus: ${bonusTokens} tokens`,
            },
            unit_amount: amount * 100,
          },
          quantity: quantity,
        },
      ],
      mode: 'payment',
      success_url: `https://beercade.com.au/checkout/success?session_id={CHECKOUT_SESSION_ID}`,
      cancel_url: `https://beercade.com.au/checkout/cancel`,
    });

    return json({ url: session.url });
  } catch (err) {
    console.error(err);
    return json({ error: 'Failed to process checkout session' }, { status: 500 });
  }
};
