import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import * as React from 'react';

import { cn } from '@/lib/utils';

const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0',
  {
    // Tessera button system — Stitch DESIGN.md §Components. Primary
    // uses a solid teal background; ghost is the default for chrome
    // (toolbars, panel headers) to keep the IDE feeling dense and
    // quiet. Outline retains a 1px border but drops the shadow to
    // sit cleanly inside compact panels.
    variants: {
      variant: {
        default:
          'bg-primary text-primary-foreground hover:bg-primary/90 active:bg-primary/85',
        secondary:
          'bg-secondary text-secondary-foreground hover:bg-secondary/85 active:bg-secondary/75',
        outline:
          'border border-border bg-card text-foreground hover:bg-muted hover:border-primary/40',
        ghost:
          'text-muted-foreground hover:bg-muted hover:text-foreground',
        destructive:
          'bg-destructive text-destructive-foreground hover:bg-destructive/85',
      },
      size: {
        default: 'h-8 px-3 py-1.5 text-xs',
        sm: 'h-7 rounded px-2.5 text-[11px]',
        lg: 'h-9 rounded px-4 text-sm',
        icon: 'h-7 w-7',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  },
);

export type ButtonProps = React.ComponentProps<'button'> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  };

/**
 * Primary interactive button primitive (shadcn/ui pattern).
 */
export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : 'button';
    return (
      <Comp className={cn(buttonVariants({ variant, size, className }))} ref={ref} {...props} />
    );
  },
);
Button.displayName = 'Button';
