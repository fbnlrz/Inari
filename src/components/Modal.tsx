import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { IconButton } from "./IconButton";

/** Centered modal dialog with a dimming scrim. Escape or scrim-click closes. */
export function Modal({
  open,
  onClose,
  title,
  children,
  className,
}: Readonly<{
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  /** Extra class on the dialog (e.g. a width variant). */
  className?: string;
}>) {
  const dialogRef = useRef<HTMLDivElement>(null);

  // Latest onClose without making it an effect dependency: callers pass an
  // inline arrow, so depending on it would re-run the effect on every parent
  // render and steal focus back into the dialog (breaking typing in dialog
  // inputs). The effect must run only when `open` flips. Same shape as Popover.
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  // Keyboard handling while open: Escape closes (like the scrim click) and
  // Tab is contained inside the dialog so focus can't wander into the UI
  // behind the scrim. Focus moves into the dialog on open and back to
  // whatever opened it on close.
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;

    dialogRef.current?.focus();

    const focusables = () =>
      Array.from(
        dialogRef.current?.querySelectorAll<HTMLElement>(
          'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCloseRef.current();
        return;
      }
      if (e.key !== "Tab") return;
      const dialog = dialogRef.current;
      if (!dialog) return;
      const items = focusables();
      if (items.length === 0) {
        e.preventDefault();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      // The dialog box itself holds focus on open but is not part of the
      // tab ring, so treat it as "outside" - otherwise Shift+Tab from it
      // walks backwards into the page behind the scrim.
      const inRing = active !== dialog && dialog.contains(active);
      if (e.shiftKey && (active === first || !inRing)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !inRing)) {
        e.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      previous?.focus?.();
    };
    // Intentionally only `open`: onClose is read via ref (see above).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  return (
    <div className="modal-scrim" onClick={onClose}>
      <div
        ref={dialogRef}
        className={"modal" + (className ? ` ${className}` : "")}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <div className="modal-title">{title}</div>
          <IconButton boxed icon="close" title="Close" onClick={onClose} size={18} />
        </div>
        {children}
      </div>
    </div>
  );
}
