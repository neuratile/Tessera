import * as React from 'react';

import { cn } from '@/lib/utils';

export type InputProps = React.InputHTMLAttributes<HTMLInputElement>;

const Input = React.forwardRef<HTMLInputElement, InputProps>(({ className, type, ...props }, ref) => {
  return (
    <input
      type={type}
      className={cn(
        // Compact density — h-8 matches the desktop button default
        // and Stitch's row-height-standard token. Drops the legacy
        // shadow + bumps focus-ring contrast so the field reads
        // clearly on the dark `bg-card` surfaces that host the
        // settings / wizard / drawer forms.
        'border-input bg-background ring-offset-background placeholder:text-muted-foreground focus-visible:ring-primary/40 focus-visible:border-primary flex h-8 w-full rounded-md border px-2.5 py-1 text-xs transition-colors file:border-0 file:bg-transparent file:text-xs file:font-medium focus-visible:outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      ref={ref}
      {...props}
    />
  );
});
Input.displayName = 'Input';

export { Input };
