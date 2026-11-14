// SPDX-License-Identifier: AGPL-3.0-or-later
// Pieces shared by the screens: headers, the public "gate" layout,
// loading and error states, and form helpers.
import { Link } from '@tanstack/react-router';
import {
  useEffect,
  useState,
  type ChangeEvent,
  type FormEvent,
  type ReactNode,
} from 'react';
import { flushSync } from 'react-dom';
import { useTranslation } from 'react-i18next';
import type { AccountKind, Permission } from '../api/client';
import { errorMessage } from '../api/errors';
import { Button } from '../components/Button';
import { Badge, Card, Notice } from '../components/Display';
import { Icon, type IconName } from '../components/Icon';

/** Sets the tab title to "<screen> · Solde". */
export function useDocumentTitle(title: string) {
  const { t } = useTranslation();
  useEffect(() => {
    document.title = t('common.documentTitle', { screen: title });
  }, [t, title]);
}

export function Wordmark({ small = false }: { small?: boolean }) {
  const { t } = useTranslation();
  return (
    <span className={`sd-wordmark ${small ? 'heading' : 'title'}`}>
      {t('common.appName')}
    </span>
  );
}

interface PageHeaderProps {
  title: string;
  overline?: ReactNode;
  subtitle?: ReactNode;
  back?:
    | { to: '/accounts'; label: string }
    | {
        to: '/accounts/$accountId';
        params: { accountId: string };
        label: string;
      };
  actions?: ReactNode;
}

export function PageHeader({
  title,
  overline,
  subtitle,
  back,
  actions,
}: PageHeaderProps) {
  useDocumentTitle(title);
  return (
    <header className="sd-page-header">
      <div className="sd-page-header__text">
        {back && (
          <Link
            to={back.to}
            params={'params' in back ? back.params : undefined}
            className="sd-back label"
          >
            <Icon name="chevron-right" size={16} />
            {back.label}
          </Link>
        )}
        {overline && <p className="overline sd-muted">{overline}</p>}
        <h1 className="title">{title}</h1>
        {subtitle && <div className="body sd-muted">{subtitle}</div>}
      </div>
      {actions && <div className="sd-row">{actions}</div>}
    </header>
  );
}

interface GateProps {
  title: string;
  intro: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
}

/** Layout of the screens shown before signing in. */
export function Gate({ title, intro, children, footer }: GateProps) {
  useDocumentTitle(title);
  return (
    <main className="sd-gate">
      <div className="sd-gate__panel">
        <div className="sd-gate__intro">
          <Wordmark />
          <h1 className="title">{title}</h1>
          <p className="body sd-muted">{intro}</p>
        </div>
        <Card as="div" className="sd-gate__card">
          {children}
        </Card>
        {footer && <div className="sd-gate__footer body">{footer}</div>}
      </div>
    </main>
  );
}

export function LoadingState() {
  const { t } = useTranslation();
  return (
    <p className="body sd-muted" role="status">
      {t('common.loading')}
    </p>
  );
}

export function ErrorState({
  error,
  onRetry,
}: {
  error: unknown;
  onRetry?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Notice tone="negative" title={t('common.loadFailed')}>
      <div className="sd-stack">
        <p>{errorMessage(t, error)}</p>
        {onRetry && (
          <div>
            <Button variant="secondary" size="sm" onClick={onRetry}>
              {t('common.retry')}
            </Button>
          </div>
        )}
      </div>
    </Notice>
  );
}

type FieldErrors<V> = Partial<Record<keyof V, string>>;

/**
 * Form state with client-side validation: `bind(field)` wires a field's
 * value, change handler and error; `handleSubmit` validates and either
 * calls `onValid` or moves focus to the first invalid field, so keyboard
 * and screen-reader users land on the error.
 */
export function useForm<V extends Record<string, string>>(
  initial: V,
  validate: (values: V) => FieldErrors<V>,
) {
  const [values, setValues] = useState<V>(initial);
  const [errors, setErrors] = useState<FieldErrors<V>>({});

  const bind = (field: keyof V) => ({
    value: values[field],
    error: errors[field],
    onChange: (event: ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
      const value = event.target.value;
      setValues((current) => ({ ...current, [field]: value }));
    },
  });

  const handleSubmit =
    (onValid: (values: V) => void) => (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const form = event.currentTarget;
      const next = validate(values);
      // Render the errors now, so the invalid field can take focus.
      flushSync(() => setErrors(next));
      if (Object.keys(next).length > 0) {
        form.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
        return;
      }
      onValid(values);
    };

  const reset = (patch: Partial<V> = {}) => {
    setValues((current) => ({ ...current, ...patch }));
    setErrors({});
  };

  return { values, errors, bind, handleSubmit, reset };
}

/** Counts user-perceived characters, like the server's 12-character rule. */
export function charCount(text: string): number {
  return [...text].length;
}

export const MIN_PASSWORD_CHARS = 12;

export function looksLikeEmail(text: string): boolean {
  return /^[^\s@]+@[^\s@]+$/.test(text.trim());
}

export const KIND_ICONS: Record<AccountKind, IconName> = {
  checking: 'building',
  savings: 'piggy',
  investment: 'chart',
  life_insurance: 'briefcase',
  loan: 'arrow-down-left',
  card: 'card',
};

export const ACCOUNT_KINDS: AccountKind[] = [
  'checking',
  'savings',
  'investment',
  'life_insurance',
  'loan',
  'card',
];

export function KindLabel({ kind }: { kind: AccountKind }) {
  const { t } = useTranslation();
  return <>{t(`accountKinds.${kind}`)}</>;
}

export function PermissionBadge({ permission }: { permission: Permission }) {
  const { t } = useTranslation();
  return <Badge>{t(`permissions.${permission}`)}</Badge>;
}

/** Last four characters of an IBAN, so lists do not show the whole number. */
export function maskIban(iban: string): string {
  const compact = iban.replace(/\s+/g, '');
  return `•••• ${compact.slice(-4)}`;
}
