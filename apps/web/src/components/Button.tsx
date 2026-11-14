// SPDX-License-Identifier: AGPL-3.0-or-later
import type { ButtonHTMLAttributes } from 'react';
import { Icon, type IconName } from './Icon';

export type ButtonVariant =
  'primary' | 'secondary' | 'ghost' | 'highlight' | 'danger';
export type ButtonSize = 'sm' | 'md' | 'lg';

/** Classes of a button, for links that look like one. */
export function buttonClass(
  variant: ButtonVariant = 'primary',
  size: ButtonSize = 'md',
  block = false,
): string {
  return [
    'sd-button',
    'label',
    `sd-button--${variant}`,
    size === 'md' ? '' : `sd-button--${size}`,
    block ? 'sd-button--block' : '',
  ]
    .filter(Boolean)
    .join(' ');
}

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  icon?: IconName;
  loading?: boolean;
  block?: boolean;
}

export function Button({
  variant = 'primary',
  size = 'md',
  icon,
  loading = false,
  block = false,
  disabled,
  className,
  children,
  type = 'button',
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={[buttonClass(variant, size, block), className]
        .filter(Boolean)
        .join(' ')}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading ? (
        <span className="sd-spinner" aria-hidden="true" />
      ) : (
        icon && <Icon name={icon} size={18} />
      )}
      {children}
    </button>
  );
}
