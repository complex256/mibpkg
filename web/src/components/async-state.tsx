import { Loader2 } from 'lucide-react';
import { Alert } from './ui/alert';

/** The car's error, in the car's own words. */
export function ErrorAlert({ error }: { error: string | null }) {
  if (!error) {
    return null;
  }
  return (
    <Alert variant="destructive" className="my-3">
      <p>{error}</p>
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
