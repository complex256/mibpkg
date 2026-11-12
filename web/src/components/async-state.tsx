import { Loader2, RotateCw } from 'lucide-react';
import { Alert } from './ui/alert';
import { Button } from './ui/button';

/** The car's error, in the car's own words, with a way to try again when one is given. */
export function ErrorAlert({ error, onRetry }: { error: string | null; onRetry?: () => void }) {
  if (!error) {
    return null;
  }
  return (
    <Alert variant="destructive" className="my-3">
      <div className="space-y-2">
        <p>{error}</p>
        {onRetry ? (
          <Button size="sm" variant="secondary" onClick={onRetry}>
            <RotateCw aria-hidden="true" />
            Try again
          </Button>
        ) : null}
      </div>
    </Alert>
  );
}

/** A quiet spinner while a page fetches its data. */
export function Loading() {
  return (
    <div className="flex items-center justify-center py-12 text-muted-foreground" role="status" aria-label="Loading">
      <Loader2 className="size-6 animate-spin" aria-hidden="true" />
    </div>
  );
}
