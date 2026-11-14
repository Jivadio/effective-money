// SPDX-License-Identifier: AGPL-3.0-or-later
// Form fields. Each one has a visible label, and its hint or error is tied
// to the control with `aria-describedby`; errors set `aria-invalid`.
import {
  forwardRef,
  useId,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
} from 'react';
import { Icon } from './Icon';

interface FieldFrameProps {
  id: string;
  label: ReactNode;
  optionalLabel?: string;
  hint?: ReactNode;
  error?: ReactNode;
  children: ReactNode;
}

function FieldFrame({
  id,
  label,
  optionalLabel,
  hint,
  error,
  children,
}: FieldFrameProps) {
  return (
    <div className="sd-field">
      <label htmlFor={id} className="sd-field__label label">
        {label}
        {optionalLabel && (
          <span className="sd-field__optional"> {optionalLabel}</span>
        )}
      </label>
      {children}
      {error ? (
        <p id={`${id}-error`} className="sd-field__error caption">
          <Icon name="alert" size={14} />
          {error}
        </p>
      ) : (
        hint && (
          <p id={`${id}-hint`} className="sd-field__hint caption">
            {hint}
          </p>
        )
      )}
    </div>
  );
}

function describedBy(id: string, hint: unknown, error: unknown) {
  if (error) return `${id}-error`;
  if (hint) return `${id}-hint`;
  return undefined;
}

interface TextFieldProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  'id'
> {
  label: ReactNode;
  /** Translated "(optional)" marker, for optional fields. */
  optionalLabel?: string;
  hint?: ReactNode;
  error?: ReactNode;
}

export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(
  function TextField({ label, optionalLabel, hint, error, ...input }, ref) {
    const id = useId();
    return (
      <FieldFrame
        id={id}
        label={label}
        optionalLabel={optionalLabel}
        hint={hint}
        error={error}
      >
        <input
          ref={ref}
          id={id}
          className="sd-input"
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy(id, hint, error)}
          {...input}
        />
      </FieldFrame>
    );
  },
);

interface SelectFieldProps extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  'id'
> {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  children: ReactNode;
}

export function SelectField({
  label,
  hint,
  error,
  children,
  ...select
}: SelectFieldProps) {
  const id = useId();
  return (
    <FieldFrame id={id} label={label} hint={hint} error={error}>
      <select
        id={id}
        className="sd-input"
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy(id, hint, error)}
        {...select}
      >
        {children}
      </select>
    </FieldFrame>
  );
}

interface AmountFieldProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  'id' | 'size'
> {
  label: ReactNode;
  /** Currency symbol or code shown before the digits, in ink-muted. */
  currencyLabel: string;
  hint?: ReactNode;
  error?: ReactNode;
}

/** Amount entry: 56px high, right-aligned `amount-lg` digits. */
export const AmountField = forwardRef<HTMLInputElement, AmountFieldProps>(
  function AmountField({ label, currencyLabel, hint, error, ...input }, ref) {
    const id = useId();
    return (
      <FieldFrame id={id} label={label} hint={hint} error={error}>
        <div
          className="sd-amount-input"
          data-invalid={error ? true : undefined}
        >
          <span className="sd-amount-input__currency label" aria-hidden="true">
            {currencyLabel}
          </span>
          <input
            ref={ref}
            id={id}
            className="amount-lg"
            inputMode="decimal"
            autoComplete="off"
            aria-invalid={error ? true : undefined}
            aria-describedby={describedBy(id, hint, error)}
            {...input}
          />
        </div>
      </FieldFrame>
    );
  },
);

interface SegmentedControlProps<V extends string> {
  label: string;
  /** Show the label above the control instead of only to screen readers. */
  showLabel?: boolean;
  options: ReadonlyArray<{ value: V; label: string }>;
  value: V;
  onChange: (value: V) => void;
}

/** An exclusive choice between 2 to 4 options (`radiogroup`). */
export function SegmentedControl<V extends string>({
  label,
  showLabel = false,
  options,
  value,
  onChange,
}: SegmentedControlProps<V>) {
  const id = useId();
  return (
    <div className="sd-field">
      <span
        id={`${id}-label`}
        className={showLabel ? 'sd-field__label label' : 'sd-visually-hidden'}
      >
        {label}
      </span>
      <div
        className="sd-segmented"
        role="radiogroup"
        aria-labelledby={`${id}-label`}
      >
        {options.map((option) => (
          <label key={option.value} className="sd-segmented__option">
            <input
              type="radio"
              name={id}
              value={option.value}
              checked={option.value === value}
              onChange={() => onChange(option.value)}
            />
            <span className="label">{option.label}</span>
          </label>
        ))}
      </div>
    </div>
  );
}
