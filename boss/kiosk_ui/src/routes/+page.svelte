<script lang="ts">
  // Modern, dark-mode neon arcade aesthetic
</script>

<svelte:head>
  <title>Beercade Jukebox</title>
</svelte:head>

<main class="jukebox-container">
  <header>
    <h1 class="neon-text">Beercade Jukebox</h1>
    <p class="subtitle">Select your game & insert coin (Scan to Pay)</p>
  </header>

  <section class="grid-container">
    <!-- Placeholder for game items -->
    {#each Array(6) as _, i}
      <button class="grid-item neon-border">
        <div class="game-info">
          <h2>Game {i + 1}</h2>
          <p>Tap to play</p>
        </div>
      </button>
    {/each}
  </section>

  <aside class="payment-section neon-border">
    <h3>Scan to Pay</h3>
    <div class="qr-placeholder">
      <!-- Stripe QR Code placeholder -->
      <div class="qr-box">
        <span>QR CODE</span>
      </div>
    </div>
    <p class="price">$1.00 per credit</p>
  </aside>
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: 'Courier New', Courier, monospace; /* Retro arcade feel */
    background-color: #0b0c10;
    color: #c5c6c7;
    overflow-x: hidden;
  }

  .jukebox-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 2rem;
    min-height: 100vh;
    box-sizing: border-box;
  }

  header {
    text-align: center;
    margin-bottom: 3rem;
  }

  .neon-text {
    font-size: 3rem;
    color: #fff;
    text-shadow:
      0 0 5px #fff,
      0 0 10px #fff,
      0 0 20px #0ff,
      0 0 40px #0ff,
      0 0 80px #0ff;
    margin: 0;
    text-transform: uppercase;
    letter-spacing: 4px;
  }

  .subtitle {
    color: #66fcf1;
    font-size: 1.2rem;
    margin-top: 0.5rem;
    letter-spacing: 2px;
  }

  .grid-container {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
    gap: 2rem;
    width: 100%;
    max-width: 1200px;
    margin-bottom: 3rem;
  }

  .grid-item {
    background: #1f2833;
    border: none;
    border-radius: 15px;
    padding: 2rem;
    color: #fff;
    cursor: pointer;
    transition: all 0.2s ease;
    display: flex;
    justify-content: center;
    align-items: center;
    text-align: center;
    min-height: 150px;
  }

  .grid-item:hover, .grid-item:active {
    transform: scale(1.05);
    background: #2a3642;
    box-shadow: 0 0 15px #66fcf1;
  }

  .neon-border {
    border: 2px solid #45a29e;
    box-shadow: inset 0 0 10px #45a29e, 0 0 10px #45a29e;
  }

  .game-info h2 {
    margin: 0 0 0.5rem 0;
    font-size: 1.5rem;
    color: #66fcf1;
  }

  .game-info p {
    margin: 0;
    color: #c5c6c7;
    font-size: 1rem;
  }

  .payment-section {
    background: #1f2833;
    padding: 2rem;
    border-radius: 15px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .payment-section h3 {
    margin: 0 0 1rem 0;
    color: #66fcf1;
    font-size: 1.5rem;
    text-transform: uppercase;
  }

  .qr-placeholder {
    background: #fff;
    padding: 10px;
    border-radius: 10px;
    margin-bottom: 1rem;
  }

  .qr-box {
    width: 150px;
    height: 150px;
    background: #000;
    display: flex;
    justify-content: center;
    align-items: center;
    color: #fff;
    font-weight: bold;
    border: 2px dashed #fff;
  }

  .price {
    margin: 0;
    font-size: 1.2rem;
    color: #fff;
    text-shadow: 0 0 5px #ff0055, 0 0 10px #ff0055;
  }

  /* Responsive touches for smaller screens */
  @media (max-width: 600px) {
    .neon-text {
      font-size: 2rem;
    }
    .grid-container {
      grid-template-columns: 1fr;
    }
  }
</style>
