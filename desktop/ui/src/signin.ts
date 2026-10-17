// Sign-in window: Google (system browser) or email code. On success it moves to linked.html.
import { core, errorText } from "./core";
import { $, followSystemTheme, fitWindowToContent } from "./page";

followSystemTheme();
fitWindowToContent($("#content"));

const views = { start: $("#view-start"), email: $<HTMLFormElement>("#view-email"), code: $<HTMLFormElement>("#view-code") };
const emailInput = $<HTMLInputElement>("#email");
const codeInput = $<HTMLInputElement>("#code");
const boxes = [...$("#code-boxes").children] as HTMLElement[];
let email = "";

function show(view: keyof typeof views): void {
  for (const [name, el] of Object.entries(views)) el.hidden = name !== view;
  if (view === "email") emailInput.focus();
  if (view === "code") codeInput.focus();
}

function setError(id: string, message: string | null): void {
  const el = $(id);
  el.textContent = message ?? "";
  el.hidden = message === null;
}

/** Disable a button while its request runs, so a double click sends one request. */
async function busy(button: HTMLButtonElement, work: () => Promise<void>): Promise<void> {
  button.disabled = true;
  try {
    await work();
  } finally {
    button.disabled = false;
  }
}

function goLinked(): void {
  window.location.replace("./linked.html");
}

// Google: the browser opens; the result comes back as an "auth-result" event.
const googleButton = $<HTMLButtonElement>("#google");
googleButton.addEventListener("click", () =>
  busy(googleButton, async () => {
    setError("#start-error", null);
    try {
      await core.signInGoogle();
    } catch (e) {
      setError("#start-error", errorText(e));
    }
  }),
);
void core.onAuthResult((result) => {
  if (result.ok) goLinked();
  else {
    show("start");
    setError("#start-error", result.message);
  }
});

$("#use-email").addEventListener("click", () => show("email"));
document.querySelectorAll("[data-back]").forEach((b) => b.addEventListener("click", () => show("start")));

async function sendCode(button: HTMLButtonElement, errorId: string): Promise<boolean> {
  let sent = false;
  await busy(button, async () => {
    setError(errorId, null);
    try {
      await core.sendEmailCode(email);
      sent = true;
    } catch (e) {
      setError(errorId, errorText(e));
    }
  });
  return sent;
}

views.email.addEventListener("submit", async (event) => {
  event.preventDefault();
  email = emailInput.value.trim();
  if (await sendCode($<HTMLButtonElement>("#send-code"), "#email-error")) {
    $("#sent-to").textContent = email;
    codeInput.value = "";
    renderCode();
    show("code");
  }
});

$<HTMLButtonElement>("#resend").addEventListener("click", (event) => {
  void sendCode(event.currentTarget as HTMLButtonElement, "#code-error");
});

/** Mirror the hidden input into the six boxes; the next empty box is the active one. */
function renderCode(): void {
  const digits = codeInput.value.replace(/\D/g, "").slice(0, 6);
  if (digits !== codeInput.value) codeInput.value = digits;
  boxes.forEach((box, i) => {
    box.textContent = digits[i] ?? "";
    box.classList.toggle("is-active", i === Math.min(digits.length, 5));
  });
}
codeInput.addEventListener("input", () => {
  renderCode();
  if (codeInput.value.length === 6) views.code.requestSubmit();
});

views.code.addEventListener("submit", (event) => {
  event.preventDefault();
  const button = $<HTMLButtonElement>("#verify");
  void busy(button, async () => {
    setError("#code-error", null);
    try {
      await core.verifyEmailCode(email, codeInput.value);
      goLinked();
    } catch (e) {
      setError("#code-error", errorText(e));
    }
  });
});

// Already signed in (window reopened): go straight to the linked screen.
void core.getState().then((state) => {
  if (state.signed_in && state.device) goLinked();
});
