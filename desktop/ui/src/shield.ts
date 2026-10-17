// The shield over a locked app: "that's your lot." and a button that asks the app to quit.
import { core, type ShieldView, type UiState } from "./core";
import { $, followSystemTheme } from "./page";

followSystemTheme();

function text(shield: ShieldView): { heading: string; body: string } {
  switch (shield.reason) {
    case "zero":
      return { heading: "that's your lot.", body: `Out of points. Earn more to use ${shield.label}.` };
    case "daily_max":
      return { heading: "that's your lot.", body: `Today's max is used up. ${shield.label} opens again tomorrow.` };
    case "offline":
      return {
        heading: "can't check your points.",
        body: `Warden has been offline too long to count your points. Connect to the internet to use ${shield.label}.`,
      };
  }
}

function render(state: UiState): void {
  const shield = state.shield;
  if (!shield) return;
  const { heading, body } = text(shield);
  $("#heading").textContent = heading;
  $("#text").textContent = body;
  $("#close-label").textContent = `Close ${shield.label}`;
}

$("#close").addEventListener("click", () => void core.closeLockedApp());
void core.onState(render);
void core.getState().then(render);
