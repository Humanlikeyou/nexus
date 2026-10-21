import uuid
import time
import threading
from typing import List, Optional, Dict
from fastapi import FastAPI, HTTPException, BackgroundTasks
from pydantic import BaseModel
from crewai import Agent, Task, Crew, Process

app = FastAPI(title="Terrance Crew Service")

# In-memory store for crew executions
crews_db: Dict[str, dict] = {}

class CrewInfo(BaseModel):
    id: str
    name: str
    status: str
    agents: List[str]
    started_at: Optional[str] = None
    task: Optional[str] = None
    result: Optional[str] = None

class CrewStartRequest(BaseModel):
    name: str
    task: str
    agents: Optional[List[str]] = None

@app.get("/health")
async def health():
    return {"status": "ok"}

@app.get("/crews/list", response_model=List[CrewInfo])
async def list_crews():
    return [CrewInfo(**c) for c in crews_db.values()]

def run_crew_task(crew_id: str, name: str, task_description: str):
    try:
        # Simple default agent if none provided
        agent = Agent(
            role="Terrance Research Assistant",
            goal=f"Execute the task: {task_description}",
            backstory="You are a precise and efficient AI assistant part of the Terrance ecosystem.",
            allow_delegation=False,
            verbose=True
        )
        
        task = Task(
            description=task_description,
            agent=agent,
            expected_output="A detailed summary of the findings."
        )
        
        crew = Crew(
            agents=[agent],
            tasks=[task],
            verbose=True,
            process=Process.sequential
        )
        
        result = crew.kickoff()
        
        crews_db[crew_id]["status"] = "completed"
        crews_db[crew_id]["result"] = str(result)
    except Exception as e:
        crews_db[crew_id]["status"] = "failed"
        crews_db[crew_id]["result"] = str(e)

@app.post("/crews/start", response_model=CrewInfo)
async def start_crew(request: CrewStartRequest, background_tasks: BackgroundTasks):
    crew_id = str(uuid.uuid4())
    started_at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    
    crew_data = {
        "id": crew_id,
        "name": request.name,
        "status": "running",
        "agents": request.agents or ["Terrance Research Assistant"],
        "started_at": started_at,
        "task": request.task,
        "result": None
    }
    
    crews_db[crew_id] = crew_data
    
    background_tasks.add_task(run_crew_task, crew_id, request.name, request.task)
    
    return CrewInfo(**crew_data)

@app.get("/crews/status", response_model=CrewInfo)
async def get_status(id: str):
    if id not in crews_db:
        raise HTTPException(status_code=404, detail="Crew execution not found")
    return CrewInfo(**crews_db[id])

@app.post("/crews/stop")
async def stop_crew(payload: dict):
    crew_id = payload.get("id")
    if not crew_id or crew_id not in crews_db:
        raise HTTPException(status_code=404, detail="Crew execution not found")
    
    if crews_db[crew_id]["status"] == "running":
        crews_db[crew_id]["status"] = "stopped"
        return {"status": "stopped"}
    else:
        return {"status": crews_db[crew_id]["status"]}

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8400)
