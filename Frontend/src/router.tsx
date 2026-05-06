import { useEffect } from "react"
import { createBrowserRouter, useNavigate } from "react-router"
import { MainPage } from "./pages/MainPage"
import { FirstRunPage } from "./pages/FirstRunPage"

function RootGuard() {
  const navigate = useNavigate()
  
  useEffect(() => {
    const isFirstRunComplete = localStorage.getItem("ide.firstRunComplete") === "true"
    if (isFirstRunComplete) {
      navigate("/ide", { replace: true })
    } else {
      navigate("/wizard", { replace: true })
    }
  }, [navigate])

  return null
}

function ErrorBoundary() {
  return (
    <div className="h-screen w-screen bg-background text-foreground flex flex-col items-center justify-center p-4">
      <h1 className="text-xl font-bold text-red-500 mb-2">Something went wrong</h1>
      <p className="text-muted-foreground">The application encountered an unexpected error.</p>
      <button 
        onClick={() => window.location.href = '/'}
        className="mt-6 bg-primary text-primary-foreground px-4 py-2 rounded-md hover:bg-primary/90"
      >
        Reload Application
      </button>
    </div>
  )
}

export const router = createBrowserRouter([
  {
    path: "/",
    element: <RootGuard />,
    errorElement: <ErrorBoundary />
  },
  {
    path: "/wizard",
    element: <FirstRunPage />,
    errorElement: <ErrorBoundary />
  },
  {
    path: "/ide",
    element: <MainPage />,
    errorElement: <ErrorBoundary />
  },
])
