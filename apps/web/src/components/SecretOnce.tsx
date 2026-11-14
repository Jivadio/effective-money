// SPDX-License-Identifier: AGPL-3.0-or-later
// A secret (invitation link, API token) the server returns only once.
import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from './Button';
import { TextField } from './Fields';

interface SecretOnceProps {
  title: string;
  label: string;
  value: string;
  children: ReactNode;
  onDone: () => void;
}

export function SecretOnce({
  title,
  label,
  value,
  children,
  onDone,
}: SecretOnceProps) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState<'idle' | 'done' | 'failed'>('idle');

  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setCopied('done');
    } catch {
      setCopied('failed');
    }
  }

  return (
    <div className="sd-secret">
      <p className="heading" role="status">
        {title}
      </p>
      <div className="body">{children}</div>
      <TextField
        label={label}
        value={value}
        readOnly
        className="sd-input sd-secret__value"
        onFocus={(event) => event.currentTarget.select()}
      />
      <div className="sd-row">
        <Button variant="primary" icon="check" onClick={() => void copy()}>
          {t('common.copy')}
        </Button>
        <Button variant="ghost" onClick={onDone}>
          {t('common.done')}
        </Button>
        <span className="caption sd-muted" aria-live="polite">
          {copied === 'done' && t('common.copied')}
          {copied === 'failed' && t('common.copyFailed')}
        </span>
      </div>
    </div>
  );
}
