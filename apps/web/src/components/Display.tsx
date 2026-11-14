// SPDX-License-Identifier: AGPL-3.0-or-later
// Read-only building blocks: Amount, Badge, Card, Notice.
import type { ReactNode } from 'react';
import { formatAmount, formatAmountParts } from '../lib/money';
import { useFormatLocale } from '../lib/locale';
import { Icon, type IconName } from './Icon';

interface AmountProps {
  /** Integer minor units. */
  value: number;
  currency: string;
  /** Minor-unit digits of `currency`: the API's `minor_digits`. */
  digits: number;
  size?: 'xl' | 'lg' | 'md' | 'sm';
  /** `+` on income, in `positive`. An ordinary expense stays in ink. */
  signed?: boolean;
  alertNegative?: boolean;
  className?: string;
}

export function Amount({
  value,
  currency,
  digits,
  size = 'md',
  signed = false,
  alertNegative = false,
  className,
}: AmountProps) {
  const locale = useFormatLocale();
  const tone =
    alertNegative && value < 0
      ? 'sd-amount--negative'
      : signed && value > 0
        ? 'sd-amount--positive'
        : '';
  const classes = ['sd-amount', `amount-${size}`, tone, className]
    .filter(Boolean)
    .join(' ');
  if (size === 'xl' || size === 'lg') {
    const parts = formatAmountParts(value, {
      locale,
      currency,
      digits,
      signed,
    });
    return (
      <span className={classes}>
        {parts.map((part, i) =>
          part.type === 'decimal' || part.type === 'fraction' ? (
            <span key={i} className="sd-amount__minor">
              {part.value}
            </span>
          ) : (
            part.value
          ),
        )}
      </span>
    );
  }
  return (
    <span className={classes}>
      {formatAmount(value, { locale, currency, digits, signed })}
    </span>
  );
}

type Tone = 'neutral' | 'positive' | 'negative' | 'warning' | 'highlight';

export function Badge({
  tone = 'neutral',
  icon,
  children,
}: {
  tone?: Tone;
  icon?: IconName;
  children: ReactNode;
}) {
  return (
    <span className={`sd-badge caption sd-badge--${tone}`}>
      {icon && <Icon name={icon} size={14} />}
      {children}
    </span>
  );
}

interface CardProps {
  title?: ReactNode;
  titleId?: string;
  action?: ReactNode;
  inverse?: boolean;
  flush?: boolean;
  as?: 'section' | 'div';
  className?: string;
  children?: ReactNode;
}

export function Card({
  title,
  titleId,
  action,
  inverse = false,
  flush = false,
  as: Tag = 'section',
  className,
  children,
}: CardProps) {
  const classes = [
    'sd-card',
    inverse ? 'sd-card--inverse' : '',
    flush ? 'sd-card--flush' : '',
    className,
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <Tag className={classes} aria-labelledby={title ? titleId : undefined}>
      {(title || action) && (
        <div className="sd-card__header">
          {title && (
            <h2 id={titleId} className="heading">
              {title}
            </h2>
          )}
          {action}
        </div>
      )}
      {children}
    </Tag>
  );
}

const NOTICE_ICONS = {
  info: 'info',
  warning: 'alert',
  negative: 'alert',
  positive: 'check',
} as const;

interface NoticeProps {
  tone?: 'info' | 'warning' | 'negative' | 'positive';
  title?: ReactNode;
  children?: ReactNode;
  /** Announce politely (`status`) even when not negative. */
  live?: boolean;
}

/** A message in the flow. Negative notices are announced as alerts. */
export function Notice({
  tone = 'info',
  title,
  children,
  live = false,
}: NoticeProps) {
  const role = tone === 'negative' ? 'alert' : live ? 'status' : undefined;
  return (
    <div className={`sd-notice sd-notice--${tone}`} role={role}>
      <Icon name={NOTICE_ICONS[tone]} size={20} />
      <div className="sd-notice__body">
        {title && <p className="label">{title}</p>}
        {children && <div className="body">{children}</div>}
      </div>
    </div>
  );
}
