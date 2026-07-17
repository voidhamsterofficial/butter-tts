<script lang="ts">
  import Butterfly from "phosphor-svelte/lib/Butterfly";
  import Rocket from "phosphor-svelte/lib/Rocket";
  import ChatCircleDots from "phosphor-svelte/lib/ChatCircleDots";
  import SlidersHorizontal from "phosphor-svelte/lib/SlidersHorizontal";
  import Lock from "phosphor-svelte/lib/Lock";
  import Bandaids from "phosphor-svelte/lib/Bandaids";
  import Command from "phosphor-svelte/lib/Command";

  // Each section gets its own colour, so the page reads as a set of cards rather than a
  // wall of text.
  const sections = [
    { icon: Butterfly, tint: "var(--butter)" },
    { icon: Rocket, tint: "var(--strawberry)" },
    { icon: Command, tint: "var(--blueberry)" },
    { icon: SlidersHorizontal, tint: "var(--mint-deep)" },
    { icon: ChatCircleDots, tint: "var(--strawberry-deep)" },
    { icon: Lock, tint: "var(--cocoa-soft)" },
    { icon: Bandaids, tint: "var(--berry-alert)" },
  ];
</script>

<div class="page__head">
  <h1 class="page__title">Docs</h1>
  <p class="page__subtitle">Everything about me, in one place.</p>
</div>

<div class="docs">
  <section class="card doc">
    <h2 class="doc__title">
      <span class="doc__icon" style="background: {sections[0].tint}">
        <Butterfly size={19} weight="fill" />
      </span>
      What is this?
    </h2>
    <p>
      Butter TTS is a <strong>voice changer for Discord</strong>. You talk into your
      microphone, and instead of your real voice going into the voice channel, I write down
      what you said and read it back out loud in a synthetic voice.
    </p>
    <p>
      Your actual microphone audio <strong>never reaches Discord</strong>. The only thing
      anyone in the channel hears is the synthesised re-reading. The recording of your voice
      exists just long enough to be transcribed, then it is thrown away — nothing is ever
      written to disk.
    </p>
    <p>
      The whole app is a single <code>butter-tts.exe</code>. There is no installer and
      nothing to uninstall. It keeps its settings and your history in two files right next
      to itself, so you can drop the lot on a USB stick and it will work the same
      elsewhere.
    </p>
  </section>

  <section class="card doc">
    <h2 class="doc__title">
      <span class="doc__icon" style="background: {sections[1].tint}">
        <Rocket size={19} weight="fill" />
      </span>
      Getting started
    </h2>
    <ol>
      <li>
        <strong>Make a Discord bot.</strong> Go to the Discord developer portal, create an
        application, add a bot to it, and copy the bot token. Invite it to your server with
        permission to join and speak in voice channels.
      </li>
      <li>
        <strong>Get an OpenAI key.</strong> From your OpenAI account page. It pays for the
        transcribing and the speaking, so it needs some credit on it.
      </li>
      <li>
        <strong>Paste both into Settings</strong>, pick your microphone, and press Save.
      </li>
      <li><strong>Press "Wake up"</strong> on the Home page. Wait for the green face.</li>
      <li>
        <strong>Type <code>/join</code></strong> in your Discord server while you are sitting
        in a voice channel. Then just talk.
      </li>
    </ol>
  </section>

  <section class="card doc">
    <h2 class="doc__title">
      <span class="doc__icon" style="background: {sections[2].tint}">
        <Command size={19} weight="fill" />
      </span>
      Discord commands
    </h2>
    <p>
      These are typed in Discord, not here. They show up after I have woken up at least
      once — Discord can take a minute to notice new commands.
    </p>
    <ul>
      <li>
        <code>/join</code> — I hop into the voice channel you are in and start listening.
        You can also name a channel: <code>/join channel:#general</code>
      </li>
      <li><code>/leave</code> — I leave and let go of your microphone.</li>
      <li><code>/voice</code> — tells you which voice I am using and where to change it.</li>
      <li><code>/ping</code> — checks I am alive.</li>
    </ul>
  </section>

  <section class="card doc">
    <h2 class="doc__title">
      <span class="doc__icon" style="background: {sections[3].tint}">
        <SlidersHorizontal size={19} weight="fill" />
      </span>
      The listening sliders
    </h2>
    <p>
      These live on the Settings page and decide how I chop your talking into sentences.
      The right numbers depend on your microphone and how noisy your room is, which is
      exactly why they are sliders and not something I guess for you.
    </p>
    <p>
      <strong>How loud counts as talking</strong> is the important one. Every microphone
      has a background hiss, and this is the line between "that is just the room" and "that
      is a person". Talk normally and watch the meter: it should shoot past the pink line
      when you speak and sit well below it when you are quiet.
    </p>
    <p>
      Set it <strong>too low</strong> and your room noise looks like non-stop talking, so I
      never notice you have finished a sentence — you get long rambling replies that only
      arrive when I hit my time limit. Set it <strong>too high</strong> and I ignore you
      unless you shout.
    </p>
    <p>
      <strong>Pause before I reply</strong> is how long you have to stop talking before I
      decide you are finished. It gets added to every reply, so it is the biggest thing
      deciding whether I feel snappy or sluggish. Too short and I cut you off mid-sentence
      whenever you pause for breath.
    </p>
    <p>
      <strong>Shortest thing worth saying</strong> filters out coughs and keyboard clacks.
      <strong>Longest I will listen</strong> is my safety net for people who never pause.
    </p>
    <p>
      <strong>Noise suppression</strong> (the toggle above the sliders) runs your
      microphone through a filter that keeps your voice and removes steady background
      noise — fans, hiss, distant chatter — before any of the above even sees it. It is on
      by default and usually worth leaving on. If it ever makes your own voice sound thin
      or clipped, turn it off; in a already-quiet room you may not need it.
    </p>
  </section>

  <section class="card doc">
    <h2 class="doc__title">
      <span class="doc__icon" style="background: {sections[4].tint}">
        <ChatCircleDots size={19} weight="fill" />
      </span>
      Your history
    </h2>
    <p>
      Every sentence I hear gets written down on the History page and kept in
      <code>butter-tts.transcripts.jsonl</code> next to the app. It is
      <strong>only the text</strong> — the audio of your voice is never saved anywhere.
    </p>
    <p>
      It keeps the most recent 10,000 things you have said, dropping the oldest after that
      so the file cannot grow forever. "Forget all" on the History page wipes it
      completely, and that cannot be undone.
    </p>
  </section>

  <section class="card doc">
    <h2 class="doc__title">
      <span class="doc__icon" style="background: {sections[5].tint}">
        <Lock size={19} weight="fill" />
      </span>
      Your keys and your privacy
    </h2>
    <p>
      Your tokens are saved in <strong>plain text</strong> in
      <code>butter-tts.settings.yaml</code>, right next to the app. That is the trade for
      being properly portable: there is no password to type and nothing hidden in the
      Windows credential store, but anyone who can read that folder can read your keys.
    </p>
    <p>
      Keep the folder somewhere you trust, and be careful putting it in a cloud-synced
      directory. If a key ever leaks, revoke it: regenerate the bot token in the Discord
      developer portal and the key in your OpenAI account.
    </p>
    <p>
      What you say is sent to <strong>OpenAI</strong> to be transcribed and spoken. Nothing
      is sent anywhere else, and there is no telemetry of any kind.
    </p>
  </section>

  <section class="card doc">
    <h2 class="doc__title">
      <span class="doc__icon" style="background: {sections[6].tint}">
        <Bandaids size={19} weight="fill" />
      </span>
      When something is wrong
    </h2>
    <p>
      <strong>The Console page is the first place to look.</strong> Everything I do lands
      there, including the exact reason anything failed.
    </p>
    <p>
      <strong>My replies ramble on and only arrive after ages.</strong> Your room noise is
      above the threshold, so I never hear you stop. Raise "how loud counts as talking".
    </p>
    <p>
      <strong>I never react at all.</strong> Either the threshold is too high, or the wrong
      microphone is picked in Settings. The meter on the Settings page tells you which.
    </p>
    <p>
      <strong>I cut you off mid-sentence.</strong> Raise "pause before I reply" so I wait
      longer through your natural pauses.
    </p>
    <p>
      <strong>"Failed to start".</strong> Almost always a token that is wrong or expired.
      The Console page will say which one Discord or OpenAI rejected.
    </p>
    <p>
      <strong>I join the channel but say nothing.</strong> Check your OpenAI account has
      credit — a rejected request shows up in the Console.
    </p>
  </section>
</div>
