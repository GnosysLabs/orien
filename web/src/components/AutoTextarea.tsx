'use client'

import { useLayoutEffect, useRef } from 'react'

/** A text box that grows with what is typed, up to `maxRows`, then
 * scrolls. It has no drag corner. */
export function AutoTextarea({
  rows = 3,
  maxRows = 14,
  className = '',
  ...props
}: React.TextareaHTMLAttributes<HTMLTextAreaElement> & { maxRows?: number }) {
  const box = useRef<HTMLTextAreaElement>(null)
  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    const style = getComputedStyle(el)
    const line = parseFloat(style.lineHeight) || 24
    const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
    const border = parseFloat(style.borderTopWidth) + parseFloat(style.borderBottomWidth)
    el.style.height = 'auto'
    const tallest = line * maxRows + padding + border
    const wanted = Math.max(el.scrollHeight + border, line * rows + padding + border)
    el.style.height = `${Math.min(wanted, tallest)}px`
    el.style.overflowY = wanted > tallest ? 'auto' : 'hidden'
  }, [props.value, rows, maxRows])
  return <textarea ref={box} rows={rows} className={`resize-none ${className}`} {...props} />
}
