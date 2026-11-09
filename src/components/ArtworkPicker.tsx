import { PanelSection, PanelSectionRow } from "@decky/ui";
import { useState } from "react";

import { artworkCandidates, applyArtworkAsset } from "../artwork";
import type { ArtworkCandidate, ArtworkCandidates } from "../backend";
import { t } from "../i18n";
import type { MessageKey } from "../i18n";
import { compactTextStyle, FocusableButton, rowStackStyle } from "../styles";
import { toastError, toastSuccess } from "../toast";

const SLOTS: Array<{ kind: keyof ArtworkCandidates; label: MessageKey; width: number; height: number }> = [
  { kind: "portrait", label: "editor.artwork.slot.portrait", width: 96, height: 144 },
  { kind: "landscape", label: "editor.artwork.slot.landscape", width: 160, height: 75 },
  { kind: "hero", label: "editor.artwork.slot.hero", width: 180, height: 58 },
  { kind: "logo", label: "editor.artwork.slot.logo", width: 140, height: 60 },
];

type ArtworkPickerProps = {
  appId: number;
  title: string;
  steamAppId: number | null;
  disabled: boolean;
};

export function ArtworkPicker({ appId, title, steamAppId, disabled }: ArtworkPickerProps) {
  const [candidates, setCandidates] = useState<Partial<ArtworkCandidates> | null>(null);
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState("");

  const load = async () => {
    if (loading || disabled) return;
    setLoading(true);
    try {
      setCandidates(await artworkCandidates(appId, title, steamAppId));
    } catch (error) {
      toastError(t("qam.artwork.title"), String(error));
    } finally {
      setLoading(false);
    }
  };

  const apply = async (kind: keyof ArtworkCandidates, candidate: ArtworkCandidate) => {
    if (applying) return;
    setApplying(candidate.url);
    try {
      await applyArtworkAsset(appId, kind, candidate.url);
      toastSuccess(t("editor.artwork.applied"), "");
    } catch (error) {
      toastError(t("qam.artwork.title"), String(error));
    } finally {
      setApplying("");
    }
  };

  return (
    <PanelSection title={t("editor.artwork.title")}>
      <PanelSectionRow>
        <FocusableButton
          className="DialogButton"
          disabled={disabled || loading}
          onClick={() => void load()}
        >
          {loading ? t("editor.artwork.loading") : t("editor.artwork.load")}
        </FocusableButton>
      </PanelSectionRow>
      {candidates
        ? SLOTS.map((slot) => {
            const items = candidates[slot.kind] ?? [];
            return (
              <PanelSectionRow key={slot.kind}>
                <div style={rowStackStyle}>
                  <div style={compactTextStyle}>{t(slot.label)}</div>
                  {items.length === 0 ? (
                    <div style={compactTextStyle}>{t("editor.artwork.empty")}</div>
                  ) : (
                    <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                      {items.map((candidate) => (
                        <FocusableButton
                          key={candidate.url}
                          className="DialogButton"
                          disabled={Boolean(applying)}
                          onClick={() => void apply(slot.kind, candidate)}
                          style={{ padding: 0, minWidth: slot.width, height: slot.height }}
                        >
                          <img
                            src={candidate.thumb || candidate.url}
                            alt=""
                            style={{ width: slot.width, height: slot.height, objectFit: "contain" }}
                          />
                        </FocusableButton>
                      ))}
                    </div>
                  )}
                </div>
              </PanelSectionRow>
            );
          })
        : null}
    </PanelSection>
  );
}
