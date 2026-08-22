import * as React from 'react'
import * as TabsPrimitive from '@radix-ui/react-tabs'

import { cn } from '@/lib/utils'

/**
 * `segmented` is the pill group used inside panels and cards — workspace tabs,
 * skill import steps, usage dimensions — where the tabs sit on a surface and
 * need their own container to read as a control.
 *
 * `underline` is the page-level form: no container, a rule across the pane, and
 * the active tab marked by a bar under its own label. Use it when the tabs are
 * the page's own sections rather than a control inside it.
 */
export type TabsVariant = 'segmented' | 'underline'

/**
 * Carried from the list to its triggers so a caller sets the variant once.
 * Triggers are always inside their list, so context is enough and the prop does
 * not have to be repeated on every tab.
 */
const TabsVariantContext = React.createContext<TabsVariant>('segmented')

const Tabs = TabsPrimitive.Root

const TabsList = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.List>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.List> & { variant?: TabsVariant }
>(({ className, variant = 'segmented', ...props }, ref) => (
  <TabsVariantContext.Provider value={variant}>
    <TabsPrimitive.List
      ref={ref}
      className={cn(
        variant === 'underline'
          ? // Scrolls rather than wraps: a second row of tabs would push the
            // content down by a line only on narrow windows.
            'flex h-9 w-full items-center justify-start gap-5 overflow-x-auto border-b border-border text-muted-foreground'
          : 'inline-flex h-9 items-center justify-center rounded-lg bg-muted p-1 text-muted-foreground',
        className,
      )}
      {...props}
    />
  </TabsVariantContext.Provider>
))
TabsList.displayName = TabsPrimitive.List.displayName

const TabsTrigger = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>
>(({ className, ...props }, ref) => {
  const variant = React.useContext(TabsVariantContext)
  return (
    <TabsPrimitive.Trigger
      ref={ref}
      className={cn(
        'inline-flex items-center justify-center whitespace-nowrap text-sm font-medium ring-offset-background transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50',
        variant === 'underline'
          ? [
              'relative h-9 shrink-0 rounded-sm px-0.5 hover:text-foreground focus-visible:ring-offset-1',
              // The bar overlaps the list's own rule rather than sitting under
              // it, so the active tab reads as cutting through the line.
              'data-[state=active]:text-foreground',
              'data-[state=active]:after:absolute data-[state=active]:after:inset-x-0',
              'data-[state=active]:after:-bottom-px data-[state=active]:after:h-0.5',
              'data-[state=active]:after:rounded-full data-[state=active]:after:bg-primary',
              'data-[state=active]:after:content-[""]',
            ]
          : 'rounded-md px-3 py-1 focus-visible:ring-offset-2 data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow',
        className,
      )}
      {...props}
    />
  )
})
TabsTrigger.displayName = TabsPrimitive.Trigger.displayName

const TabsContent = React.forwardRef<
  React.ComponentRef<typeof TabsPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TabsPrimitive.Content>
>(({ className, ...props }, ref) => (
  <TabsPrimitive.Content
    ref={ref}
    className={cn(
      'mt-2 ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2',
      className,
    )}
    {...props}
  />
))
TabsContent.displayName = TabsPrimitive.Content.displayName

export { Tabs, TabsContent, TabsList, TabsTrigger }
